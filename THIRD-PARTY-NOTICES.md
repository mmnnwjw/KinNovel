# Third-party notices

KinNovel is distributed under the GNU General Public License version 3 (or later).
The complete license text is included in `LICENSE`.

## Statically linked native code

### FBInk

- URL: https://github.com/NiLuJe/FBInk
- Version: commit `886f25f` (2026-08-06), built with `MINIMAL=1 INPUT=1` and linked
  statically into `bin/kinnovel`.
- Used for: e-ink display updates on every supported Kindle (waveforms, MTK/mxcfb
  quirks, device identification) and input device discovery.
- License: GPL-3.0-or-later. Source: the `rust/third_party/FBInk` submodule of this
  repository, or upstream at the commit above.

## Data

### rime-pinyin-simp (pinyin dictionary)

- URL: https://github.com/rime/rime-pinyin-simp, commit `0c6861e` (2024-12-29); itself
  derived from the Android Open Source Project's PinyinIME.
- Used for: the pinyin input of the on-screen keyboard (search). Converted by
  `tools/gen_pinyin.py` and embedded (Brotli-compressed) in `bin/kinnovel`.
- License: Apache-2.0. The license text is in `LICENSE-rime-pinyin-simp` (release zip) /
  `rust/crates/kn-ime/data/LICENSE-rime-pinyin-simp` (source).

The keyboard's layout and input behaviour follow KOReader's `VirtualKeyboard` and pinyin
keyboard; no KOReader code is included.

## Rust crates

`bin/kinnovel` is a single static binary built from the crates under `rust/` and the
crates below (the complete runtime dependency graph of the device build). All of them
are under permissive licenses compatible with GPL-3.0; their license texts are in the
crate sources published on crates.io.

| Crate | Version | License |
|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause |
| alloc-stdlib | 0.2.4 | BSD-3-Clause |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| bitflags | 1.3.2 | MIT/Apache-2.0 |
| bitvec | 1.1.1 | MIT |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| brotli | 7.0.0 | BSD-3-Clause AND MIT |
| brotli-decompressor | 4.0.3 | BSD-3-Clause/MIT |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT |
| byteorder-lite | 0.1.0 | Unlicense OR MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| bytes | 1.12.1 | MIT |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| data-encoding | 2.11.1 | MIT |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| equivalent | 1.0.2 | Apache-2.0 OR MIT |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| font-types | 0.9.0 | MIT OR Apache-2.0 |
| four-cc | 0.4.0 | MIT/Apache-2.0 |
| funty | 2.0.0 | MIT |
| generic-array | 0.14.7 | MIT |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| html5ever | 0.39.0 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| image-webp | 0.2.4 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| jpeg-decoder | 0.3.2 | MIT OR Apache-2.0 |
| libc | 0.2.190 | MIT OR Apache-2.0 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| log | 0.4.34 | MIT OR Apache-2.0 |
| markup5ever | 0.39.0 | MIT OR Apache-2.0 |
| markup5ever_rcdom | 0.39.0+unofficial | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| new_debug_unreachable | 1.0.6 | MIT |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| paste | 1.0.15 | MIT OR Apache-2.0 |
| phf | 0.13.1 | MIT |
| phf_shared | 0.13.1 | MIT |
| png | 0.17.16 | MIT OR Apache-2.0 |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 |
| precomputed-hash | 0.1.1 | MIT |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quick-error | 2.0.1 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| radium | 0.7.0 | MIT |
| rand | 0.8.8 | MIT OR Apache-2.0 |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 |
| read-fonts | 0.29.3 | MIT OR Apache-2.0 |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| rustls | 0.23.45 | Apache-2.0 OR ISC OR MIT |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 |
| rustls-webpki | 0.103.15 | ISC |
| safer-bytes | 0.2.0 | MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 |
| sha1 | 0.10.7 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| siphasher | 1.0.4 | MIT OR Apache-2.0 |
| skrifa | 0.31.3 | MIT OR Apache-2.0 |
| smallvec | 1.16.3 | MIT OR Apache-2.0 |
| string_cache | 0.9.0 | MIT OR Apache-2.0 |
| subtle | 2.6.1 | BSD-3-Clause |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.6 | MIT OR Apache-2.0 |
| tap | 1.0.1 | MIT |
| tendril | 0.5.1 | MIT OR Apache-2.0 |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |
| tungstenite | 0.24.0 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| untrusted | 0.9.0 | ISC |
| utf-8 | 0.7.6 | MIT OR Apache-2.0 |
| web_atoms | 0.2.6 | MIT OR Apache-2.0 |
| webpki-roots | 0.26.11 | CDLA-Permissive-2.0 |
| webpki-roots | 1.0.9 | CDLA-Permissive-2.0 |
| woff2-patched | 0.4.0 | Apache-2.0 |
| wyz | 0.5.1 | MIT |
| xml5ever | 0.39.0 | MIT OR Apache-2.0 |
| zeno | 0.3.3 | Apache-2.0 OR MIT |
| zerocopy | 0.8.62 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zeroize | 1.9.1 | Apache-2.0 OR MIT |
| zlib-rs | 0.6.8 | Zlib |
| zmij | 1.0.23 | MIT |

`webpki-roots` embeds the Mozilla CA certificate list (CDLA-Permissive-2.0); it is used
to verify the TLS certificates of the LightNovelShelf servers.

## Reference projects (no code copied)

- **LightNovelShelf/Web** — https://github.com/LightNovelShelf/Web — API contracts,
  authentication flow, reading-position semantics and chapter font behaviour.
- **KOReader** — https://github.com/koreader/koreader (AGPL-3.0) — Kindle device facts
  (framework suspend/resume, lipc power events, per-model input quirks). Only behaviour
  and facts were used; no source was translated or copied.
- **kComics** — https://github.com/lxdklp/kComics (GPL-3.0) — Kindle launcher lifecycle
  and framebuffer handling used by the earlier Python line (0.x).

## Fonts

KinNovel does not ship fonts. It uses the Kindle's system font
(`/usr/java/lib/fonts/STHeitiMedium.ttf`) for the interface and the per-chapter fonts
served by LightNovelShelf for chapter text; both stay on the device.

## Content notice

KinNovel accesses only the public LightNovelShelf web interfaces. Novel text,
covers, fonts and other content remain subject to the rights of their
respective owners and to the service's terms. Users are responsible for
following applicable content licenses and site rules.
