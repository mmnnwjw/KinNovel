# KinNovel implementation guide

## Runtime entry

- `bin/app.py` assembles configuration, API client, screen output, evdev input
  and the page registry.
- `bin/start.sh` pauses Kindle UI processes, snapshots `/dev/fb0`, starts the
  app, then restores the previous screen and processes on exit.
- `bin/src/screen/` is the adapted kComics framebuffer/evdev layer.
- `bin/src/page/keyboard.py` is the adapted pinyin-capable touch keyboard.

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

## Data flow

### Login

`account.py` collects credentials with the on-device keyboard. `api.py` sends
SHA-256 password data to `/api/user/login`, stores the returned access and
refresh tokens, then invokes `GetMyInfo`. The token provider refreshes the
access token before Hub calls when it is older than 25 seconds.

### Search and catalogue

Page modules call typed `ApiClient` methods. The client serializes parameters
and invokes the Hub with:

```json
[params, {"UseGzip": true}]
```

The response may be a JSON object or a base64-encoded gzip JSON byte array.
`SignalRClient` handles both forms.

### Reading

`reader.py` page code calls `GetNovelContent`. The response chapter is passed to
`ReaderDocument`, which:

1. Parses a safe HTML subset.
2. Downloads and loads `Chapter.Font`.
3. Measures and wraps text with that exact Pillow font.
4. Creates page command lists.
5. Maps the current page back to a relative XPath for `SaveReadPosition`.

The screen renders one page of text/images at a time. Page turns are local;
crossing the first or last page switches chapters.

### Shelf

The client reads the server's flat shelf array. Folders are selected through
the `parents` path. Adding/removing books and creating/deleting folders rewrites
the array and calls `SaveBookShelf`. `index` is normalized locally on creation;
the server remains the source of truth.

## Implemented API families

- REST authentication, email codes, registration and password reset.
- Catalogue: latest, paged list, categories, all search dimensions, series,
  ranking, book details.
- Reading: chapter content, progress, history, per-book position.
- User: profile/growth, notifications, daily sign-in.
- Shelf: fetch and save.
- Comments: list and post.
- Shop: catalogue, owned items and purchase.
- Direct-message methods are present in the API client but have no UI.

## Excluded or degraded

- Upload, publish, edit, delete and reorder.
- Whole-book/chapter download and EPUB/CBZ/MOBI export.
- Forum/community.
- Manga image reader and comic quota flows.
- Direct-message conversation UI.
- Avatar upload and profile editing.

These exclusions avoid Calibre, large decoded image buffers, complex editor
state and interactions that are poor on a six-inch e-ink touch screen.

## Verification

```sh
PYTHONPATH=bin/src python -m unittest discover -s tests -v
python tools/render_preview.py
python -m compileall -q bin
```

The render tool produces `build/previews/home.png`, `browse.png` and
`reader.png` using a fake screen; it does not require `/dev/fb0`.

Authenticated live tests are kept separate because they use credentials and
must be rate-limited:

- `tools/live_account_smoke.py` performs small serial requests with a
  10-second timer after every network step.
- `tools/live_font_probe.py` fetches one chapter and compares chapter-font and
  system-font rendering without printing the chapter text.
