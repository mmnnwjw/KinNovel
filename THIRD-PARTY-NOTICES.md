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

### FBInk

- URL: https://github.com/NiLuJe/FBInk
- Used for: Kindle EPDC driver interface references, hardware platform quirks
  (MTK, Rex, Zelda, and legacy MXCFB), ambient temperature constants, and
  screen update alignment specifications.
- License: AGPLv3 / GPLv3.

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

## Bundled native libraries (`bin/lib`)

The shared objects under `bin/lib` are the armhf Kindle runtime. Except for
`bin/lib/freetype-woff2`, they come from the Debian jessie-era armhf Kindle
runtime used by `kComics`. `bin/lib/freetype-woff2` is the KOReader
`v2026.03` runtime for WOFF2 chapter fonts. KinNovel does not patch these
binaries; they are redistributed unmodified. Corresponding sources are
available from the upstream projects below.

| File | Component | Version | License | Source |
|---|---|---|---|---|
| `libcrypto.so.3` | OpenSSL | 3.6.1 | Apache-2.0 | https://github.com/openssl/openssl |
| `libssl.so.3` | OpenSSL | 3.6.1 | Apache-2.0 | https://github.com/openssl/openssl |
| `libxml2.so.2` | libxml2 | 2.9.14 | MIT | https://gitlab.gnome.org/GNOME/libxml2 |
| `libxslt.so.1` | libxslt | 1.1.42 | MIT | https://gitlab.gnome.org/GNOME/libxslt |
| `libexslt.so.0` | libxslt (EXSLT) | 1.1.42 | MIT | https://gitlab.gnome.org/GNOME/libxslt |
| `libz.so.1` | zlib | 1.2.8 | zlib | https://zlib.net |
| `libjpeg.so.62` | libjpeg-turbo (libjpeg 6.2 ABI) | ABI 6.2 | IJG / BSD-3-Clause | https://github.com/libjpeg-turbo/libjpeg-turbo |
| `libtiff.so.5` | libtiff | 4.0.3 | libtiff (BSD-style) | https://gitlab.com/libtiff/libtiff |
| `libwebp.so.7` | libwebp | 1.3.2 | BSD-3-Clause | https://chromium.googlesource.com/webm/libwebp |
| `libwebpdemux.so.2` | libwebp demux | 1.3.2 | BSD-3-Clause | https://chromium.googlesource.com/webm/libwebp |
| `libwebpmux.so.3` | libwebp mux | 1.3.2 | BSD-3-Clause | https://chromium.googlesource.com/webm/libwebp |
| `libsharpyuv.so.0` | libwebp sharpyuv | 1.3.2 | BSD-3-Clause | https://chromium.googlesource.com/webm/libwebp |
| `libjbig.so.0` | JBIG-KIT | 2.1 | GPLv2 | https://www.cl.cam.ac.uk/~mgk25/jbigkit/ |
| `libopenjp2.so.7` | OpenJPEG | 2.5.0 | BSD-2-Clause | https://github.com/uclouvain/openjpeg |
| `liblzma.so.5` | XZ Utils (liblzma) | 5.1.0alpha | Public Domain | https://tukaani.org/xz/ |
| `libffi.so.6` | libffi | 3.x (soname 6) | MIT | https://github.com/libffi/libffi |
| `libfreetype.so.6` | FreeType | 2.12.1 | FTL / GPLv2 | https://freetype.org |
| `freetype-woff2/libfreetype.so.6` | FreeType (WOFF2 build) | KOReader v2026.03 | GPLv3 | https://github.com/koreader/koreader |
| `freetype-woff2/libz.so.1` | zlib (WOFF2 build) | 1.3.2 | zlib | https://zlib.net |

Version evidence: version strings embedded in the binaries (for example
`OpenSSL 3.6.1`, `LIBTIFF, Version 4.0.3`, `freetype-2.12.1`,
`/root/libxml2-2.9.14`, `/root/libxslt-1.1.42`, `JBIG-KIT 2.1`,
`openjpeg` `2.5.0`, `webp-1.3.2`). `libffi` only exposes soname 6 and
`libjpeg.so.62` only exposes the libjpeg 6.2 ABI symbol, so no exact upstream
version can be recovered from the file itself.

SHA-256 (`bin/lib`, computed from this release tree):

| File | SHA-256 |
|---|---|
| `libcrypto.so.3` | `d8ee41ff528ab36b1037f5c27068c42c9908274f58e28e9b4b7b6a1f3242ff83` |
| `libexslt.so.0` | `5481702fab241e40504ab0fe111a22b62e7cebf272adddde92814fa37ff54773` |
| `libffi.so.6` | `39e40c959e000021b6bc35f05d408d75924d7c6fe95fd2a80a52aabc2012e86f` |
| `libfreetype.so.6` | `c2553861000845cfd6aec2e5a0106b9c1a552024e796bedd7fe563621acf3778` |
| `libjbig.so.0` | `199d989987b1af98385f70377ddcb372ed6f8c5d324177f66933d13e4ef04b95` |
| `libjpeg.so.62` | `4e02d736a79d768ec3c9d9ac9f759e35e0d4d9c898817e07363b91a73e47e995` |
| `liblzma.so.5` | `05c96e72458cb5060f24e412af2e74b8d30ff8f08d0cb483d671cacb49ea01f4` |
| `libopenjp2.so.7` | `e78985bcf24eaa5148d47c5972a7090da691208fb60d0391e9887f2c9bd07449` |
| `libsharpyuv.so.0` | `57856da168f0818d558035aa9bfdd6f2cb5f365b80f734f52026b7d4f75c1e33` |
| `libssl.so.3` | `8f77fd780589c2bfcfb8a4d72079ab79098963cd23f8e73ac735b979d712b0c3` |
| `libtiff.so.5` | `d1ef86b6a4b7bb0f07e38bffa352f59eedbf0ac668c962475fa1fc44f009f634` |
| `libwebp.so.7` | `12340d83aaf60cfc74599b773a8e105a9b6e26fccf31dd6fb1cabd8c132fd17d` |
| `libwebpdemux.so.2` | `365e869900b4bf57d04fc760eaaac906d897b64f54a652eb98c00524075e728b` |
| `libwebpmux.so.3` | `90b03ee03138ff938067033bd701580f2d10816816d1355c923212200c920030` |
| `libxml2.so.2` | `2b06bb6b0dac79439faa63fd2b83a71d4c4c71b8b906db2a88a13c2e3b3acfd2` |
| `libxslt.so.1` | `12b2c81b211b35f35f54c1dcfd670c245635634be1f5cfe07b1ac0b2124f1a47` |
| `libz.so.1` | `5b600414650ac305c93875512a49c6857ab6b7d0c2d831763b1cad12b7ec9158` |
| `freetype-woff2/libfreetype.so.6` | `cdc5afddf765d49069c5ab3ceb8c63ca815545ae772cbe741869c657115a5294` |
| `freetype-woff2/libz.so.1` | `79a78432f05a2dff2db4e518a13827c7979aeed10b1fa7cdc9aa0e350e78416b` |

### OpenSSL NOTICE

This product includes software developed by the OpenSSL Project for use in
the OpenSSL Toolkit (https://www.openssl.org/). OpenSSL 3.x is distributed
under the Apache License 2.0.

## Bundled Python packages (`bin/vendor`)

| Package | Version | License | Source | Notes |
|---|---|---|---|---|
| Pillow | 12.3.0 | MIT-CMU | https://github.com/python-pillow/Pillow | armhf CPython 3.14 build; includes the C extensions used for framebuffer rendering, image decoding and FreeType text |
| lxml | 6.1.1 | BSD-3-Clause | https://github.com/lxml/lxml | armhf CPython 3.14 build; libxml2/libxslt linked from the bundled runtime above |
| python-evdev | not embedded in the binary | BSD-3-Clause | https://github.com/gvalkov/python-evdev | armhf CPython 3.14 build; upstream source is the acquisition path |

The vendored packages keep their upstream `LICENSE` files (`bin/vendor/lxml/`
and `bin/vendor/evdev/`); Pillow's license text is the MIT-CMU license in the
upstream project. KinNovel does not carry local patches to these packages.

## Content notice

KinNovel accesses only the public LightNovelShelf web interfaces. Novel text,
covers, fonts and other content remain subject to the rights of their
respective owners and to the service's terms. Users are responsible for
following applicable content licenses and site rules.
