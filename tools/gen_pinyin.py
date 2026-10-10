"""Build the pinyin dictionary embedded in the on-screen keyboard (rust/crates/kn-ime).

Source: rime-pinyin-simp (https://github.com/rime/rime-pinyin-simp, Apache-2.0,
itself derived from AOSP PinyinIME). KOReader's pinyin keyboard uses the same
source (single characters only); we keep the words too.

Usage:
    git clone --depth 1 https://github.com/rime/rime-pinyin-simp.git /path/to/rime-pinyin-simp
    python tools/gen_pinyin.py /path/to/rime-pinyin-simp/pinyin_simp.dict.yaml

Output: rust/crates/kn-ime/data/pinyin.br, Brotli-compressed UTF-8 text, one entry per line:
    <syllables joined by '>\t<word>\t<weight>
sorted by key (bytewise), then by weight (descending). Lookups binary-search the keys.
"""

import re
import sys
from pathlib import Path

import brotli

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "rust" / "crates" / "kn-ime" / "data" / "pinyin.br"
CODE = re.compile(r"^[a-z]+( [a-z]+)*$")


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    text = Path(sys.argv[1]).read_text(encoding="utf-8")
    body = text.split("\n...\n", 1)[1]
    entries = {}
    for line in body.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 2:
            continue
        word, code = parts[0], parts[1].strip()
        weight = int(parts[2]) if len(parts) > 2 and parts[2].strip() else 0
        if not CODE.match(code) or "\t" in word or not word:
            continue
        key = code.replace(" ", "'")
        if len(word) != code.count(" ") + 1:
            continue  # one syllable per character; skip the few mixed entries
        prev = entries.get((key, word))
        if prev is None or weight > prev:
            entries[(key, word)] = weight
    rows = sorted(entries.items(), key=lambda kv: (kv[0][0].encode(), -kv[1], kv[0][1]))
    out = "".join(f"{key}\t{word}\t{weight}\n" for (key, word), weight in rows)
    raw = out.encode("utf-8")
    packed = brotli.compress(raw, quality=11, lgwin=24)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(packed)
    syllables = {k for (key, _), _ in rows for k in key.split("'")}
    chars = sum(1 for (key, _), _ in rows if "'" not in key)
    print(f"{len(rows)} entries ({chars} single characters, {len(syllables)} syllables), "
          f"{len(raw)} bytes -> {len(packed)} bytes: {OUT}")


if __name__ == "__main__":
    main()
