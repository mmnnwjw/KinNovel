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

## Second review round (2026-09)

| Severity | Finding | Resolution |
|---|---|---|
| P0 | `browse` page 2+ rendered blank: server-paginated `Data` was indexed by the global `(page-1)*per_page` offset. | Row index is now the in-page index; numbering keeps the global offset for display only. Regression test added. |
| P0 | Reader chapter load had no staleness check: switching chapters quickly let an older response overwrite the newer chapter state. | `enter` success/error closures now verify `book_id`/`sort_num` before applying. |
| P0 | The `mxcfb` protocol used NXP upstream numbers (ioctl `0x46`, 64-byte struct), which no Kindle lab126 kernel accepts. | Struct, ioctls (`0x2E/0x2F/0x30/0x37`), waveform and flag enums now follow KOReader `mxcfb-kindle.h` (68-byte struct with `hist_bw/hist_gray` fields). |
| P1 | EPDC region alignment floored width/height, so e.g. 758px-wide screens never refreshed the right 6 pixels. | Start aligns down, end aligns up, then clamps to screen bounds. |
| P1 | `auto` protocol detection always picked `mtk` because ioctl errors were swallowed, leaving mxcfb devices with a dead screen. | Initialization now submits a small probe update; failure falls through to the next protocol and closes the fd. |
| P1 | `ApiError` (business failure, 401) was retried like a transport error, closing a healthy socket and doubling rate-limit pressure. | `invoke` re-raises `ApiError` without retry. Test added. |
| P1 | A SignalR record split across two WebSocket messages failed JSON parsing and the completion was lost. | Records now accumulate in a byte buffer split on `0x1e`. Test added. |
| P1 | `wrap_line` moved trailing closing punctuation to the start of the next line (inverted kinsoku rule). | Closing punctuation now squeezes onto the current line; opening punctuation moves to the next line. Tests added. |
| P1 | Chapter font cache used the URL basename, so same-named fonts overwrote each other, and truncated downloads were cached permanently. | Cache key is now a URL hash; downloads validate `Content-Length` before persisting. |
| P1 | Announcements list had no pager (pages > 1 unreachable) and shared the browse indexing bug. | Local indexing plus a bottom pager; empty state added. |
| P1 | Book detail `bound` state raced between books and compared `int` against raw shelf ids. | Result guarded by `book_id`; ids normalized to `int`. |
| P1 | Long-press triggered tap actions everywhere (including exiting the app from home). | Pages that do not use long-press now ignore non-tap gestures. |
| P1 | History could only be cleared via an invisible top-right hotspot, and rows beyond the first screen were unreachable. | Bottom bar with visible pager and explicit "清空" button. |
| P2 | Shop items beyond index 8 were unreachable. | Dynamic rows per screen height plus a pager. |
| P2 | gzip responses had no decompression limit. | Streaming gunzip with an 8MB cap. Test added. |
| P2 | WebSocket handshake did not validate `Sec-WebSocket-Accept`; handshake socket timeouts bypassed retry logic. | Accept hash verified; handshake errors wrapped as `TransportError`. |
| P2 | Successive toasts raced and cleared each other early. | Toast clearing is generation-guarded. |
| P2 | Reader re-ran LANCZOS contain on every render for the same illustration. | Fitted images cached per (url, size) with a small bound. |
| P2 | Headings used the body line height and could overlap. | Line height now derives from each run's font size. |
| P2 | `FontResolver` bypassed the system font cache; glyph cache grew unbounded. | System fonts cached per size; glyph cache capped. |
| P2 | Touch edge pixels mapped to `render_w` (out of range); scanned evdev devices leaked fds; press durations between tap and long thresholds were dropped. | Pixel clamp, scan handles closed, duration dead zone removed. |
| P2 | `fb_snapshot restore` crashed when the snapshot was larger than `smem_len`. | Writes truncate to the smaller length. |
| P2 | `write_image` copied row-by-row even for full-screen writes; `show(region=...)` wrote the whole image at the region origin; log file grew forever; framebuffer fd/mmap were never closed. | Single-slice fast path, region cropping, startup log rotation, `close()` on shutdown. |
| P2 | Shelf folder deletion copy implied the contents would be deleted; the actual (spec-correct) behavior promotes contents to the parent level. | Confirm copy updated to match behavior. |

Deferred: main-loop rendering still runs synchronously inside the evdev read
loop; ioctl timeout threads can linger in the kernel; partial/dirty-region
refresh is not yet wired into `PageContext.show()`. These need on-device
validation before changing.
