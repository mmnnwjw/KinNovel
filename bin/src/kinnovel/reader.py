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
from .utils import absolute_url, atomic_write


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


def _plain_text(element):
    parts = []
    for node in element.iter():
        tag = str(node.tag).lower() if isinstance(node.tag, str) else ""
        if tag in ("br",):
            parts.append("\n")
        if node.text:
            parts.append(node.text)
        if tag in ("img",):
            alt = node.get("alt")
            if alt:
                parts.append(alt)
        if node.tail:
            parts.append(node.tail)
    return re.sub(r"[ \t\r\f\v]+", " ", "".join(parts)).strip()


def extract_blocks(content, base_url=""):
    root = sanitize_html(content)
    blocks = []
    seen = set()
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        if element is root:
            continue
        tag = element.tag.lower()
        if element in seen:
            continue
        if tag == "img":
            src = absolute_url(base_url, element.get("src") or element.get("data-system-image-url") or "")
            if src:
                blocks.append(Block("image", source_url=src, path=_relative_xpath(element, root)))
            seen.add(element)
            continue
        if tag not in BLOCK_TAGS:
            continue
        if any(isinstance(child.tag, str) and child.tag.lower() in BLOCK_TAGS
               for child in element):
            continue
        direct_text = _plain_text(element)
        if not direct_text:
            continue
        kind = "heading" if tag in HEADING_TAGS else ("footnote" if tag == "aside" else "text")
        level = int(tag[1]) if kind == "heading" else 0
        blocks.append(Block(kind, text=direct_text, level=level,
                            path=_relative_xpath(element, root)))
        seen.add(element)
    if not blocks:
        text = _plain_text(root)
        if text:
            blocks.append(Block("text", text=text, path="."))
    return blocks


def ensure_font(font_url, base_url, timeout=30, strict_tls=False):
    if not font_url:
        return ""
    url = absolute_url(base_url, font_url)
    name = re.sub(r"[^A-Za-z0-9_.-]", "_", url.split("?", 1)[0].rsplit("/", 1)[-1]) or "chapter-font"
    path = CACHE_DIR / "fonts" / name
    if path.exists() and path.stat().st_size > 0:
        return str(path)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "KinNovel/0.1"})
        context = None
        if url.startswith("https://"):
            import ssl
            context = ssl.create_default_context()
            if not strict_tls:
                context.check_hostname = False
                context.verify_mode = ssl.CERT_NONE
        with urllib.request.urlopen(request, timeout=timeout, context=context) as response:
            data = response.read(20 * 1024 * 1024)
        if data:
            atomic_write(path, data)
            converted = normalize_font(path)
            return str(converted or path)
    except (OSError, urllib.error.URLError):
        return ""
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
                payload = zlib.decompress(payload)
            if len(payload) != original_length:
                return None
            tables.append((tag, checksum, payload))
        tables.sort(key=lambda item: item[0])
        entry_selector = int(math.log(max(1, num_tables), 2))
        search_range = (2 ** entry_selector) * 16
        range_shift = num_tables * 16 - search_range
        header = struct.pack(">IHHHH", struct.unpack(">I", flavor)[0], num_tables,
                             search_range, entry_selector, range_shift)
        offset = 12 + num_tables * 16
        records = []
        payloads = []
        for tag, checksum, payload in tables:
            records.append(struct.pack(">4sIII", tag, checksum, offset, len(payload)))
            payloads.append(payload)
            offset += len(payload)
            offset += (-offset) % 4
        suffix = ".otf" if flavor == b"OTTO" else ".ttf"
        target = path.with_suffix(path.suffix + suffix)
        atomic_write(target, header + b"".join(records) + b"".join(payloads))
        return target
    except (OSError, ValueError, struct.error, zlib.error):
        return None


class FontResolver:
    def __init__(self, system_font):
        self.system_font = system_font
        self._cache = {}
        self.custom_font_loaded = False
        self.last_error = ""

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
        try:
            return ImageFont.truetype(self.system_font, int(size))
        except OSError:
            return ImageFont.load_default()


def text_width(draw, text, font):
    try:
        return float(font.getlength(text))
    except AttributeError:
        bbox = draw.textbbox((0, 0), text, font=font)
        return float(bbox[2] - bbox[0])


def wrap_line(draw, text, font, max_width, remove_trailing_spaces=True):
    text = str(text or "").replace("\u00a0", " ")
    if not text:
        return [""]
    lines = []
    current = ""
    current_width = 0.0
    for character in text:
        if character == "\n":
            lines.append(current.rstrip() if remove_trailing_spaces else current)
            current = ""
            current_width = 0.0
            continue
        character_width = text_width(draw, character, font)
        if current and current_width + character_width > max_width:
            cut = current
            if len(cut) > 1 and cut[-1] in "，。！？；：、,.!?;:'\")]】》”’":
                cut = cut[:-1]
                current = current[-1] + character
                current_width = text_width(draw, current, font)
            else:
                current = character
                current_width = character_width
            lines.append(cut.rstrip() if remove_trailing_spaces else cut)
        else:
            current += character
            current_width += character_width
    lines.append(current.rstrip() if remove_trailing_spaces else current)
    return lines


class ReaderDocument:
    def __init__(self, chapter, base_url, system_font, config):
        self.chapter = chapter or {}
        self.base_url = base_url
        self.config = config
        self.blocks = extract_blocks(self.chapter.get("Content") or "", base_url)
        self.font_resolver = FontResolver(system_font)
        self.body_font = None
        self.small_font = None
        self.pages = []
        self._draw_proxy = None

    def prepare(self, draw, width, height):
        from PIL import ImageDraw
        if self._draw_proxy is None:
            image = __import__("PIL.Image", fromlist=["Image"]).new("L", (8, 8), 255)
            self._draw_proxy = ImageDraw.Draw(image)
        size = int(self.config.get("font_size") or 34)
        self.body_font = self.font_resolver.resolve(
            self.chapter.get("Font"),
            self.base_url,
            size,
            strict_tls=bool(self.config.get("strict_tls")),
        )
        self.small_font = self.font_resolver.resolve(
            self.chapter.get("Font"),
            self.base_url,
            max(20, int(size * 0.82)),
            strict_tls=bool(self.config.get("strict_tls")),
        )
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

        def add_line(text, font=None, indent=False, gap_before=0, gap_after=0):
            nonlocal y, current
            font = font or self.body_font
            if y + gap_before + line_height > usable_height and current:
                new_page()
            y += gap_before
            prefix = "　　" if indent and self.config.get("first_line_indent") else ""
            for line in wrap_line(draw, prefix + text, font, usable_width):
                if y + line_height > usable_height and current:
                    new_page()
                current.append({
                    "type": "text",
                    "text": line,
                    "x": margin,
                    "y": margin + y,
                    "font": font,
                    "size": getattr(font, "size", 28),
                    "path": path,
                })
                y += line_height
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
                })
                y += image_height + int(line_height * 0.5)
                continue
            if block.kind == "footnote":
                footnote_lines.append((block.text, path))
                continue
            if block.kind == "heading":
                font = self.font_resolver.resolve(
                    self.chapter.get("Font"),
                    self.base_url,
                    int(self.body_font.size * max(1.08, 1.30 - block.level * 0.05)),
                    strict_tls=bool(self.config.get("strict_tls")),
                )
                add_line(block.text, font=font, indent=False,
                         gap_before=heading_gap if current else 0,
                         gap_after=int(line_height * 0.35))
            else:
                add_line(block.text, indent=self.config.get("first_line_indent", True),
                         gap_after=int(line_height * 0.22))
        if footnote_lines:
            add_line("注释", font=self.small_font, gap_before=heading_gap)
            for text, path in footnote_lines:
                add_line(text, font=self.small_font, gap_after=int(line_height * 0.15))
        if current or not pages:
            pages.append(current)
        return pages

    def page_for_path(self, xpath):
        if not xpath:
            return 0
        for index, page in enumerate(self.pages):
            if any(item.get("path") == xpath for item in page):
                return index
        return 0

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
