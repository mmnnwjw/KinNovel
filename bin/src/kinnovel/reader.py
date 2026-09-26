import html
import math
import re
import struct
import urllib.error
import urllib.request
import zlib
from dataclasses import dataclass, field

from lxml import etree

from .config import CACHE_DIR
from .transport import TransportError
from .utils import absolute_url, atomic_write, stable_cache_name


BLOCK_TAGS = {
    "p", "div", "section", "article", "blockquote", "h1", "h2", "h3",
    "h4", "h5", "h6", "li", "dt", "dd", "pre", "figcaption", "aside",
}
HEADING_TAGS = {"h1", "h2", "h3", "h4", "h5", "h6"}
SKIP_TAGS = {"script", "style", "iframe", "object", "embed", "svg", "canvas"}


@dataclass
class Block:
    kind: str
    text: str = ""
    level: int = 0
    path: str = "."
    offset: int = 0
    source_url: str = ""
    footnotes: list = field(default_factory=list)


def sanitize_html(content):
    parser = etree.HTMLParser(recover=True, no_network=True, encoding="utf-8")
    try:
        root = etree.fromstring(("<div id='kinnovel-root'>" + str(content or "") + "</div>").encode("utf-8"),
                                parser=parser)
    except (etree.ParserError, ValueError):
        root = etree.Element("div")
    wrapper = root.find(".//*[@id='kinnovel-root']")
    if wrapper is not None:
        root = wrapper
    for element in list(root.iter()):
        tag = str(element.tag).lower() if isinstance(element.tag, str) else ""
        if tag in SKIP_TAGS:
            parent = element.getparent()
            if parent is not None:
                parent.remove(element)
            continue
        for attribute in list(element.attrib):
            lower = attribute.lower()
            if lower.startswith("on") or lower in ("style", "srcset"):
                del element.attrib[attribute]
        if tag == "a":
            href = element.get("href", "")
            if href.lower().startswith(("javascript:", "data:")):
                del element.attrib["href"]
    return root


def _relative_xpath(element, root):
    if element is root:
        return "."
    if element.get("id"):
        return '//*[@id="%s"]' % element.get("id").replace('"', '\\"')
    steps = []
    current = element
    while current is not root and current is not None:
        tag = str(current.tag).lower()
        parent = current.getparent()
        if parent is None:
            break
        index = 1
        for sibling in parent:
            if sibling is current:
                break
            if str(sibling.tag).lower() == tag:
                index += 1
        steps.append("%s[%d]" % (tag, index))
        current = parent
    steps.reverse()
    return "./" + "/".join(steps) if steps else "."


# 零宽/格式字符:在浏览器中不渲染,直接剥除,避免回退字体画出方框
_INVISIBLE_RE = re.compile("[\u200b\u200c\u200d\ufeff\u00ad\u2060]")


def _clean_text(text):
    text = _INVISIBLE_RE.sub("", str(text or ""))
    return re.sub(r"[ \t\r\f\v]+", " ", text).strip()


def _plain_text(element):
    parts = []
    for node in element.iter():
        tag = str(node.tag).lower() if isinstance(node.tag, str) else ""
        if tag in ("br",):
            parts.append("\n")
        if node.text:
            parts.append(node.text)
        if node.tail:
            parts.append(node.tail)
    return _clean_text("".join(parts))


def extract_blocks(content, base_url=""):
    root = sanitize_html(content)
    blocks = []

    def flush_text(buffer, offset, kind, level, path):
        text = _clean_text("".join(buffer))
        del buffer[:]
        if not text:
            return offset
        blocks.append(Block(kind, text=text, level=level, path=path,
                            offset=offset))
        return offset + len(text)

    def append_node(node, kind, level, path, buffer, offset):
        if node.text:
            buffer.append(node.text)
        for child in node:
            if not isinstance(child.tag, str):
                continue
            tag = child.tag.lower()
            if tag == "img":
                offset = flush_text(buffer, offset, kind, level, path)
                src = absolute_url(
                    base_url,
                    child.get("src") or child.get("data-system-image-url") or "",
                )
                if src:
                    blocks.append(Block(
                        "image", path=_relative_xpath(child, root),
                        offset=offset, source_url=src,
                    ))
            elif tag == "br":
                buffer.append("\n")
            elif tag in BLOCK_TAGS:
                offset = flush_text(buffer, offset, kind, level, path)
                child_kind = ("heading" if tag in HEADING_TAGS else
                              ("footnote" if tag == "aside" else "text"))
                child_level = int(tag[1]) if child_kind == "heading" else 0
                child_path = _relative_xpath(child, root)
                child_offset = append_node(
                    child, child_kind, child_level,
                    child_path, buffer, 0,
                )
                flush_text(
                    buffer, child_offset, child_kind, child_level, child_path)
            else:
                offset = append_node(child, kind, level, path, buffer, offset)
            if child.tail:
                buffer.append(child.tail)
        return offset

    buffer = []
    offset = append_node(root, "text", 0, ".", buffer, 0)
    flush_text(buffer, offset, "text", 0, ".")
    if not blocks:
        text = _plain_text(root)
        if text:
            blocks.append(Block("text", text=text, path="."))
    return blocks


def ensure_font(font_url, base_url, timeout=30, strict_tls=False):
    if not font_url:
        return ""
    url = absolute_url(base_url, font_url)
    suffix = ""
    if "." in url.split("?", 1)[0].rsplit("/", 1)[-1]:
        suffix = "." + url.split("?", 1)[0].rsplit(".", 1)[-1].lower()
    if suffix not in (".ttf", ".otf", ".woff", ".woff2"):
        suffix = ".font"
    path = CACHE_DIR / "fonts" / (stable_cache_name(url) + suffix)
    if path.exists() and path.stat().st_size > 0:
        converted = normalize_font(path)
        return str(converted or path)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "KinNovel/0.1"})
        context = None
        if url.startswith("https://"):
            import ssl
            context = ssl.create_default_context()
            if not strict_tls:
                context.check_hostname = False
                context.verify_mode = ssl.CERT_NONE
        limit = 20 * 1024 * 1024
        with urllib.request.urlopen(request, timeout=timeout, context=context) as response:
            expected = response.headers.get("Content-Length")
            data = response.read(limit + 1)
        if not data or len(data) > limit:
            return ""
        if expected is not None and int(expected) != len(data):
            return ""
        atomic_write(path, data)
        converted = normalize_font(path)
        return str(converted or path)
    except (OSError, urllib.error.URLError, ValueError):
        return ""


def normalize_font(path):
    """Convert an uncompressed-table WOFF1 container to TTF/OTF for Pillow."""
    path = __import__("pathlib").Path(path)
    try:
        data = path.read_bytes()
    except OSError:
        return None
    if not data.startswith(b"wOFF"):
        return path
    if len(data) < 44:
        return None
    try:
        flavor = data[4:8]
        num_tables = struct.unpack(">H", data[12:14])[0]
        if num_tables <= 0 or num_tables > 128 or len(data) < 44 + num_tables * 20:
            return None
        tables = []
        for index in range(num_tables):
            offset = 44 + index * 20
            tag, table_offset, compressed_length, original_length, checksum = struct.unpack(
                ">4sIIII", data[offset:offset + 20])
            if table_offset + compressed_length > len(data):
                return None
            payload = data[table_offset:table_offset + compressed_length]
            if compressed_length < original_length:
                decompressor = zlib.decompressobj()
                payload = decompressor.decompress(payload, original_length + 1)
                if len(payload) > original_length:
                    return None
                if decompressor.unconsumed_tail:
                    remaining = original_length + 1 - len(payload)
                    payload += decompressor.decompress(
                        decompressor.unconsumed_tail, remaining)
                if len(payload) > original_length:
                    return None
                remaining = original_length + 1 - len(payload)
                payload += decompressor.flush(max(1, remaining))
            if len(payload) != original_length:
                return None
            tables.append((tag, checksum, payload))
        tables.sort(key=lambda item: item[0])
        entry_selector = int(math.log(max(1, num_tables), 2))
        search_range = (2 ** entry_selector) * 16
        range_shift = num_tables * 16 - search_range
        header = struct.pack(">IHHHH", struct.unpack(">I", flavor)[0], num_tables,
                             search_range, entry_selector, range_shift)
        body = bytearray()
        records = []
        for tag, checksum, payload in tables:
            table_offset = 12 + num_tables * 16 + len(body)
            records.append(struct.pack(
                ">4sIII", tag, checksum, table_offset, len(payload)))
            body.extend(payload)
            body.extend(b"\0" * ((-len(body)) % 4))
        suffix = ".otf" if flavor == b"OTTO" else ".ttf"
        target = path.with_suffix(path.suffix + suffix)
        atomic_write(target, header + b"".join(records) + bytes(body))
        return target
    except (OSError, ValueError, struct.error, zlib.error):
        return None


class FontResolver:
    def __init__(self, system_font_path):
        self.system_font_path = system_font_path
        self._cache = {}
        self.custom_font_loaded = False
        self.last_error = ""
        self._system_cache = {}

    def system_font(self, size):
        from PIL import ImageFont
        key = int(size)
        if key not in self._system_cache:
            try:
                self._system_cache[key] = ImageFont.truetype(
                    self.system_font_path, key)
            except OSError:
                self._system_cache[key] = ImageFont.load_default()
        return self._system_cache[key]

    def resolve(self, chapter_font, base_url, size, strict_tls=False):
        from PIL import ImageFont
        if chapter_font:
            key = (chapter_font, int(size))
            if key not in self._cache:
                path = ensure_font(chapter_font, base_url, strict_tls=strict_tls)
                try:
                    self._cache[key] = ImageFont.truetype(path, int(size)) if path else None
                except OSError as exc:
                    self._cache[key] = None
                    self.last_error = str(exc)
            if self._cache.get(key) is not None:
                self.custom_font_loaded = True
                return self._cache[key]
        return self.system_font(int(size))


def text_width(draw, text, font):
    try:
        return float(font.getlength(text))
    except AttributeError:
        bbox = draw.textbbox((0, 0), text, font=font)
        return float(bbox[2] - bbox[0])


_GLYPH_CACHE = {}
_GLYPH_CACHE_LIMIT = 40000
_NOTDEF_BYTES = {}


def _char_bitmap_bytes(font, character):
    from PIL import Image, ImageDraw
    size = int(getattr(font, "size", 48) or 48)
    image = Image.new("L", (max(8, size * 2), max(8, int(size * 1.6))), 0)
    ImageDraw.Draw(image).text((0, 0), character, font=font, fill=255)
    return image.tobytes()


def _notdef_bytes(font):
    key = id(font)
    if key not in _NOTDEF_BYTES:
        _NOTDEF_BYTES[key] = _char_bitmap_bytes(font, "\U0010FFFF")
    return _NOTDEF_BYTES[key]


def glyph_available(font, character):
    if not character:
        return True
    if character.isspace():
        return True
    key = (id(font), character)
    cached = _GLYPH_CACHE.get(key)
    if cached is not None:
        return cached
    try:
        mask = font.getmask(character)
        available = bool(mask.getbbox())
        if available:
            try:
                if _char_bitmap_bytes(font, character) == _notdef_bytes(font):
                    # 缺字时部分 FreeType 渲染出带轮廓的 notdef 方框(如 STHeiti),
                    # 与 .notdef 位图一致即视为缺字,交给回退链处理
                    available = False
            except (AttributeError, OSError, TypeError, ValueError):
                pass
    except (AttributeError, OSError, ValueError):
        available = False
    if len(_GLYPH_CACHE) >= _GLYPH_CACHE_LIMIT:
        _GLYPH_CACHE.clear()
    _GLYPH_CACHE[key] = available
    return available


def char_font(font, fallback_font, character):
    if fallback_font is None or glyph_available(font, character):
        return font
    if glyph_available(fallback_font, character):
        return fallback_font
    return font


def split_font_runs(text, font, fallback_font):
    runs = []
    for character in str(text or ""):
        selected = char_font(font, fallback_font, character)
        if runs and runs[-1][1] is selected:
            runs[-1] = (runs[-1][0] + character, selected)
        else:
            runs.append((character, selected))
    return runs


# 标点禁则:闭标点不允许出现在行首,开标点不允许出现在行尾
_LINE_START_FORBIDDEN = "，。、；：？！,.!?;:'\")]】》”’%…—·"
_LINE_END_FORBIDDEN = "（《【「『“‘([{"


def _wrap_line_parts(draw, text, font, max_width, fallback_font=None,
                     remove_trailing_spaces=True):
    text = str(text or "").replace("\u00a0", " ")
    if not text:
        return [("", 0)]
    lines = []
    current = ""
    current_start = 0
    current_width = 0.0

    def measure(value):
        return sum(
            text_width(draw, character,
                       char_font(font, fallback_font, character))
            for character in value
        )

    def rendered(value):
        return value.rstrip() if remove_trailing_spaces else value

    index = 0
    while index < len(text):
        character = text[index]
        if character == "\n":
            lines.append((rendered(current), current_start))
            current = ""
            current_width = 0.0
            index += 1
            current_start = index
            continue
        if character in _LINE_START_FORBIDDEN:
            run_end = index + 1
            while (run_end < len(text) and
                   text[run_end] in _LINE_START_FORBIDDEN):
                run_end += 1
            run = text[index:run_end]
            run_width = measure(run)
            if current and current_width + run_width > max_width:
                if len(run) == 1:
                    # 保留单字符闭标点的悬挂语义
                    current += run
                    lines.append((rendered(current), current_start))
                    current = ""
                    current_width = 0.0
                    index = run_end
                    continue
                cut = current
                carry_start = len(cut)
                while (carry_start > 0 and
                       cut[carry_start - 1] in _LINE_END_FORBIDDEN):
                    carry_start -= 1
                carry = cut[carry_start:]
                cut = cut[:carry_start]
                if cut:
                    lines.append((rendered(cut), current_start))
                current_start += carry_start
                current = carry + run
                current_width = measure(current)
            else:
                if not current:
                    current_start = index
                current += run
                current_width += run_width
            index = run_end
            continue
        character_width = text_width(
            draw, character, char_font(font, fallback_font, character))
        if current and current_width + character_width > max_width:
            cut = current
            carry_start = len(cut)
            while (carry_start > 0 and
                   cut[carry_start - 1] in _LINE_END_FORBIDDEN):
                carry_start -= 1
            carry = cut[carry_start:]
            cut = cut[:carry_start]
            if cut:
                lines.append((rendered(cut), current_start))
            current_start += carry_start
            current = carry + character
            current_width = measure(current)
        else:
            if not current:
                current_start = index
            current += character
            current_width += character_width
        index += 1
    lines.append((rendered(current), current_start))
    return lines


def wrap_line(draw, text, font, max_width, fallback_font=None,
              remove_trailing_spaces=True):
    return [
        line for line, _offset in _wrap_line_parts(
            draw, text, font, max_width,
            fallback_font=fallback_font,
            remove_trailing_spaces=remove_trailing_spaces,
        )
    ]


class ReaderDocument:
    def __init__(self, chapter, base_url, system_font, config):
        self.chapter = chapter or {}
        self.base_url = base_url
        self.config = config
        self.blocks = extract_blocks(self.chapter.get("Content") or "", base_url)
        self.font_resolver = FontResolver(system_font)
        self.body_font = None
        self.body_fallback = None
        self.small_font = None
        self.small_fallback = None
        self.pages = []
        self._draw_proxy = None

    def prepare(self, draw, width, height):
        from PIL import ImageDraw
        if self._draw_proxy is None:
            image = __import__("PIL.Image", fromlist=["Image"]).new("L", (8, 8), 255)
            self._draw_proxy = ImageDraw.Draw(image)
        size = int(self.config.get("font_size") or 48)
        self.body_font = self.font_resolver.resolve(
            self.chapter.get("Font"),
            self.base_url,
            size,
            strict_tls=bool(self.config.get("strict_tls")),
        )
        self.body_fallback = self.font_resolver.system_font(size)
        self.small_font = self.font_resolver.resolve(
            self.chapter.get("Font"),
            self.base_url,
            max(20, int(size * 0.82)),
            strict_tls=bool(self.config.get("strict_tls")),
        )
        self.small_fallback = self.font_resolver.system_font(max(20, int(size * 0.82)))
        self.pages = self._paginate(self._draw_proxy, int(width), int(height))
        return self.pages

    def _paginate(self, draw, width, height):
        margin = int(self.config.get("reader_margin") or 34)
        usable_width = max(120, width - 2 * margin)
        usable_height = max(160, height - 2 * margin)
        line_height = max(1, int(self.body_font.size * float(self.config.get("line_spacing") or 1.42)))
        heading_gap = int(line_height * 0.5)
        footnote_lines = []
        pages = []
        current = []
        y = 0

        def new_page():
            nonlocal current, y
            if current:
                pages.append(current)
            current = []
            y = 0

        def add_line(text, font=None, fallback_font=None, indent=False,
                     gap_before=0, gap_after=0, base_offset=0):
            nonlocal y, current
            font = font or self.body_font
            fallback_font = fallback_font or self.body_fallback
            # 标题等大字号行使用自身行高,避免与正文行高不一致导致重叠
            actual_height = max(line_height, int(
                getattr(font, "size", 28) * float(self.config.get("line_spacing") or 1.42)))
            if y + gap_before + actual_height > usable_height and current:
                new_page()
            y += gap_before
            prefix = "　　" if indent and self.config.get("first_line_indent") else ""
            for line, line_start in _wrap_line_parts(
                    draw, prefix + text, font, usable_width,
                    fallback_font=fallback_font):
                if y + actual_height > usable_height and current:
                    new_page()
                current.append({
                    "type": "text",
                    "text": line,
                    "x": margin,
                    "y": margin + y,
                    "font": font,
                    "fallback_font": fallback_font,
                    "size": getattr(font, "size", 28),
                    "path": path,
                    "offset": base_offset + max(0, line_start - len(prefix)),
                })
                y += actual_height
            y += gap_after

        for block in self.blocks:
            path = block.path
            if block.kind == "image":
                image_height = min(int(usable_height * 0.62), int(usable_width * 0.72))
                if y + image_height > usable_height and current:
                    new_page()
                current.append({
                    "type": "image",
                    "url": block.source_url,
                    "x": margin,
                    "y": margin + y,
                    "width": usable_width,
                    "height": image_height,
                    "path": path,
                    "offset": block.offset,
                })
                y += image_height + int(line_height * 0.5)
                continue
            if block.kind == "footnote":
                footnote_lines.append((block.text, path, block.offset))
                continue
            if block.kind == "heading":
                font = self.font_resolver.resolve(
                    self.chapter.get("Font"),
                    self.base_url,
                    int(self.body_font.size * max(1.08, 1.30 - block.level * 0.05)),
                    strict_tls=bool(self.config.get("strict_tls")),
                )
                add_line(block.text, font=font,
                         fallback_font=self.font_resolver.system_font(
                             int(self.body_font.size * max(
                                 1.08, 1.30 - block.level * 0.05))),
                         indent=False,
                         gap_before=heading_gap if current else 0,
                         gap_after=int(line_height * 0.35),
                         base_offset=block.offset)
            else:
                add_line(block.text, indent=self.config.get("first_line_indent", True),
                         gap_after=int(line_height * 0.22),
                         base_offset=block.offset)
        if footnote_lines:
            add_line("注释", font=self.small_font,
                     fallback_font=self.small_fallback, gap_before=heading_gap)
            for text, path, offset in footnote_lines:
                add_line(text, font=self.small_font,
                         fallback_font=self.small_fallback,
                         gap_after=int(line_height * 0.15),
                         base_offset=offset)
        if current or not pages:
            pages.append(current)
        return pages

    def page_for_path(self, xpath, offset=None):
        if not xpath:
            return 0
        if offset is not None:
            try:
                target_offset = int(offset)
            except (TypeError, ValueError):
                target_offset = None
            if target_offset is not None:
                first_page = None
                best_page = None
                best_offset = None
                for index, page in enumerate(self.pages):
                    for item in page:
                        if item.get("path") != xpath:
                            continue
                        if first_page is None:
                            first_page = index
                        item_offset = int(item.get("offset") or 0)
                        if item_offset > target_offset:
                            continue
                        if best_offset is None or item_offset > best_offset:
                            best_page = index
                            best_offset = item_offset
                if best_page is not None:
                    return best_page
                if first_page is not None:
                    return first_page
                return 0
        for index, page in enumerate(self.pages):
            if any(item.get("path") == xpath for item in page):
                return index
        return 0

    def first_anchor_on_page(self, page_index):
        if not self.pages:
            return (".", 0)
        page_index = max(0, min(int(page_index), len(self.pages) - 1))
        for item in self.pages[page_index]:
            if item.get("path"):
                return item["path"], int(item.get("offset") or 0)
        return (".", 0)

    def first_path_on_page(self, page_index):
        if not self.pages:
            return "."
        page_index = max(0, min(int(page_index), len(self.pages) - 1))
        for item in self.pages[page_index]:
            if item.get("path"):
                return item["path"]
        return "."

    @property
    def page_count(self):
        return max(1, len(self.pages))

    @property
    def title(self):
        return str(self.chapter.get("Title") or "未命名章节")

    def chapters(self):
        return self.chapter.get("Chapters") or []
