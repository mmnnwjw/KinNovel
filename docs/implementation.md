# KinNovel implementation guide

## Runtime entry

- `bin/app.py` assembles configuration, API client, screen output, evdev input
  and the page registry.
- `bin/start.sh` pauses Kindle UI processes, snapshots `/dev/fb0`, starts the
  app, then restores the previous screen and processes on exit.
- `bin/src/screen/` is the adapted kComics framebuffer/evdev layer.

## Application modules

| Module | Responsibility |
|---|---|
| `kinnovel/config.py` | JSON configuration and writable application paths |
| `kinnovel/utils.py` | atomic writes, cache pruning, password hashing and formatting |
| `kinnovel/transport.py` | REST helper, rate limiter, RFC 6455 WebSocket and SignalR |
| `kinnovel/api.py` | LightNovelShelf REST/Hub domain API |
| `kinnovel/reader.py` | HTML subset, chapter font, WOFF conversion, wrapping, pagination and XPath |
| `kinnovel/ui.py` | framebuffer canvas, list/dialog widgets, navigation, image cache |
| `kinnovel/pages/` | home, catalogue, search, book detail, reader, shelf, account and settings pages |

`bin/image_worker.py` is a legacy standalone helper kept for compatibility; the
active image pipeline is the bounded worker pool and priority queue inside
`kinnovel/ui.py`.

## Data flow

### Login

`account.py` reads credentials from `bin/config.json`, then `api.py` sends
SHA-256 password data to `/api/user/login`, stores the returned access and
refresh tokens, and invokes `GetMyInfo`. The token provider refreshes the
access token before Hub calls when it is older than 25 seconds.

### Search and catalogue

Page modules call typed `ApiClient` methods. The client serializes parameters
and invokes the Hub with:

```json
[params, {"UseGzip": true}]
```

The response may be a JSON object or a base64-encoded gzip JSON byte array.
`SignalRClient` handles both forms.

### SignalR transport

- One WebSocket, JSON hub protocol. Calls are still serialized on the wire, but
  a priority turn scheduler lets interactive calls go before background chapter
  prefetch (`priority=1`).
- A protocol Ping (type 6) is sent when the connection would otherwise idle
  out, so a long reading session does not pay a fresh negotiate/TLS/WebSocket
  handshake on the next call.
- Read-only calls with identical `(method, params)` are coalesced; categories
  are cached for 5 minutes and announcement lists for 1 minute.
- `shutdown()` interrupts a blocked receive with `socket.shutdown()` before
  closing, so app exit does not wait for a hung network call.
- Connect, handshake and per-invocation receive each have their own timeout
  budget; the record buffer is capped at 16MB.

### Reading

`reader.py` page code calls `GetNovelContent`. The response chapter is passed to
`ReaderDocument`, which:

1. Parses a safe HTML subset.
2. Downloads and loads `Chapter.Font`.
3. Measures and wraps text with that exact Pillow font.
4. Builds `ReaderDocument.pages`, a list of per-page item dicts
   (`text`/`image` entries with position, font, path and offset).
5. Maps the current page back to a relative XPath for `SaveReadPosition`.

The screen renders one page of text/images at a time. Page turns are local;
crossing the first or last page switches chapters.

Opening a book detail page does not prefetch chapter content by default.
`prefetch_reading_target` (Settings: "详情页预热") must be enabled explicitly
because the server may record the request as a reading event.

### Shelf

The client reads the server's flat shelf array. Folders are selected through
the `parents` path. Adding/removing books and creating/deleting folders rewrites
the array and calls `SaveBookShelf`. `index` is normalized locally on creation;
the server remains the source of truth.

### List page position

`PageContext.returning` is true only when `enter()` runs because the user pressed
back. List pages (shelf, history, rank, browse, series and announcements) use it
to keep the previous page; a fresh navigation from home resets to page one.

## Implemented API families

- REST authentication, email codes, registration and password reset.
- Catalogue: paged list, categories, ranking and book details.
- Reading: chapter content, progress, history, per-book position.
- User: profile/growth, notifications, daily sign-in.
- Shelf: fetch and save.
- Comments: read-only listing.
- Shop: catalogue, owned items and purchase.

## Excluded or degraded

- Upload, publish, edit, delete and reorder.
- Whole-book/chapter download and EPUB/CBZ/MOBI export.
- Forum/community.
- Manga image reader and comic quota flows.
- Text-entry screens, direct messages and the manga reader.
- Avatar upload and profile editing.

These exclusions avoid Calibre, large decoded image buffers, complex editor
state and interactions that are poor on a six-inch e-ink touch screen.

## Verification

```sh
PYTHONPATH=bin/src python -m unittest discover -s tests -v
python tools/render_preview.py
python -m compileall -q bin
```

The render tool produces `build/previews/home.png`, `browse.png`, `reader.png`,
`settings.png` and `about.png` using a fake screen; it does not require
`/dev/fb0`.

Authenticated live tests are kept separate because they use credentials and
must be rate-limited:

- `tools/live_account_smoke.py` performs small serial requests with a
  10-second timer after every network step.
- `tools/live_font_probe.py` fetches one chapter and compares chapter-font and
  system-font rendering without printing the chapter text.
