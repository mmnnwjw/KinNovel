# Third-party notices

KinNovel is distributed under the GNU General Public License version 3.
The complete license text is included in `LICENSE`.

## Reference projects

### LightNovelShelf/Web

- URL: https://github.com/LightNovelShelf/Web
- Used for: API contracts, authentication flow, reader behavior, reading
  position semantics, and chapter font behavior.
- Implementation: KinNovel reimplements the Kindle client in Python. The
  Quasar/Vue application source is not included.

### kComics

- URL: https://github.com/lxdklp/kComics
- Used for: Kindle framebuffer and EPDC output, MTK/MXCFB refresh handling,
  evdev touch parsing, launcher lifecycle, system process pause/resume, screen
  snapshot, and Python packaging layout.
- License: GPLv3.

### KOReader

- URL: https://github.com/koreader/koreader
- Version: v2026.03.
- Redistributed files:
  - `bin/lib/freetype-woff2/libfreetype.so.6`
    SHA-256 `cdc5afddf765d49069c5ab3ceb8c63ca815545ae772cbe741869c657115a5294`
  - `bin/lib/freetype-woff2/libz.so.1`
    SHA-256 `79a78432f05a2dff2db4e518a13827c7979aeed10b1fa7cdc9aa0e350e78416b`
- Purpose: FreeType/Brotli support required to load LightNovelShelf WOFF2
  chapter fonts in the bundled Kindle Pillow build.
- License: GPLv3.
- These files are used first at runtime. If they are missing, KinNovel falls
  back to `/mnt/us/koreader/libs/libfreetype.so.6`.
- Corresponding KOReader source is available from the URL above and at the
  repository release matching version v2026.03.

## Bundled or adapted components

| Component | Purpose | License | URL |
|---|---|---|---|
| Pillow | Framebuffer rendering, image decoding, FreeType fonts | MIT-CMU | https://github.com/python-pillow/Pillow |
| lxml | HTML parsing for the novel reader | BSD | https://github.com/lxml/lxml |
| python-evdev | Kindle touch input | BSD | https://github.com/gvalkov/python-evdev |

## Content notice

KinNovel accesses only the public LightNovelShelf web interfaces. Novel text,
covers, fonts and other content remain subject to the rights of their
respective owners and to the service's terms. Users are responsible for
following applicable content licenses and site rules.
