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
- Used for: a compatible FreeType runtime with WOFF2/Brotli support when the
  device already has KOReader installed.
- Runtime path: `/mnt/us/koreader/libs/libfreetype.so.6`.
- KOReader source code is not included in the KinNovel package.

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
