# Code review and resolution

The implementation was reviewed by a separate agent with a focus on bugs,
Kindle resource use, and missing tests. The initial findings and their current
resolution are recorded below.

| Severity | Finding | Resolution |
|---|---|---|
| P0 | Keyboard expected integer font keys while the app exposed named keys. | `bin/app.py` now exposes both key sets; a keyboard render test was added. |
| P1 | All non-tap gestures were discarded globally, so shelf long-press actions were dead. | `PageContext` now forwards `tap` and `long`; a long-gesture test was added. |
| P1 | Header back/home labels had no hit handling. | Header left and default right actions are handled centrally; page-specific right actions remain page-owned. |
| P1 | Several list pages requested more rows than they rendered and advanced the server page, skipping data. | Request sizes now match visible page sizes; notifications received previous/next controls. |
| P1 | Reader pagination height differed from the actual rendered content height. | Pagination and clipping now use the same header/content/footer geometry. |
| P1 | Clearing cache removed `session.json` and `visitor-id`. | Cache clearing now targets only covers, fonts, images, and content directories. |
| P1 | TLS verification was disabled by default. | `strict_tls` now defaults to `true`; disabling it remains an explicit compatibility option. |
| P2 | A WebSocket frame arriving with the HTTP 101 response could be dropped. | Remaining bytes after the HTTP headers are retained in the socket receive buffer. |
| P2 | Older asynchronous responses could overwrite newer user selections. | Catalogue, search, series, rank, announcement, book and notification loads use generation checks. |
| Perf | Cache pruning repeatedly rescanned the tree and the limit was not applied. | Pruning scans once per cache directory and runs on a background thread at startup. |
| Perf | Missing covers/images could block the render thread. | Image drawing no longer performs synchronous downloads; prefetch is asynchronous. |
| Perf | Character-by-character wrapping recalculated the full line width. | Wrapping now accumulates per-character width. A 112,007-character benchmark dropped to 0.593 seconds on the host. |

## Verification after fixes

```text
python -m unittest discover -s tests -v
Ran 16 tests: OK

python -m compileall -q bin
OK

python tools/render_preview.py
home.png, browse.png, reader.png generated

Live Hub GetLatestBookList with strict TLS:
Total=11568, returned=6
```

True-device framebuffer/EPDC, evdev coordinate mapping, Kindle sleep/resume and
the full authenticated API flow still require testing on a jailbroken device.

## Live account smoke test

The workspace test account was used with a dedicated 10-second-per-request
script. The test covered login, `GetMyInfo`, shelf, history, latest books, book
details, one real chapter, the chapter font, announcements and notifications.
It remained serial and did not request images or batches.

- Login and authenticated Hub calls succeeded.
- The selected real chapter contained 521 characters.
- The live chapter font was a 1,058,952-byte WOFF2 file.
- Host Pillow 11.3 loaded the WOFF2 directly and rendered correct Chinese
  glyphs; the bundled Kindle Pillow is version 12.3 with FreeType and needs
  final on-device WOFF2 confirmation.
- The font contained no PUA code points, but the chapter-font and system-font
  renders differed by about 7.94% of pixels on the same text sample, proving
  that the visual mapping comes from the chapter font itself.
- One long request caused the server to close the WebSocket before the next
  call; this exposed and fixed a missing reconnect-and-retry path.
- The final full run completed without authorization or rate-limit errors.
