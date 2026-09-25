# LightNovelShelf and kComics technical analysis

This document is based on the source versions below:

- `LightNovelShelf/Web`: commit `cd1b4b79fe7face28cb70ae77f2a1ba288cf3cfa`
  (`fix: 书架按类型筛选时递归过滤并隐藏空文件夹`).
- `lxdklp/kComics`: commit `904852fcb4f932f84e4b26ce92506a1d3e1278e4`
  (`perf: 内存优化`).

## 1. LightNovelShelf architecture

LightNovelShelf/Web is a Quasar/Vue 3 SPA. Its runtime depends on two transports:

- REST calls under `/api/...` for authentication and file downloads.
- ASP.NET Core SignalR at `/hub/api` for catalogue, reading, shelf, comments,
  notifications, points, shop and direct-message business methods.

The SPA persists settings locally, keeps the refresh token in IndexedDB, keeps
the short-lived access token in memory, and reconnects the Hub after login or
token changes.

### 1.1 HTTP envelope and headers

Every JSON REST response uses:

```json
{
  "Success": true,
  "Response": {},
  "Status": 200,
  "Msg": ""
}
```

The client must:

- send `Accept: application/json`;
- send a stable `x-id` value;
- use JSON for POST and query parameters for GET;
- treat `Success == false` or a non-2xx status as an API error.

The web client's request queue permits at most 9 Hub calls in a 5.5 second
window. The Kindle client preserves the same limit.

Authorization is enforced by the Hub method, not only by the SPA route.
`GetLatestBookList`, announcements and ranking are usable anonymously. On the
current live server, `GetBookList`, detailed search, `GetBookCategories`,
`GetBookInfo`, `GetNovelContent`, shelf and account methods return
`user is unauthorized` without an access token.

### 1.2 SignalR protocol details

Negotiation:

```text
POST {apiServer}/hub/api/negotiate?negotiateVersion=1
```

The response contains `connectionToken`. The WebSocket URL is:

```text
wss://{host}/hub/api?id={connectionToken}
```

The JSON protocol handshake is:

```json
{"protocol":"json","version":1}
```

Every SignalR frame is suffixed with the ASCII record separator `0x1e`.

The web code exposes:

```ts
invokeHub(methodName, params, options = { UseGzip: true })
```

It invokes the Hub with two arguments:

```json
[params, {"UseGzip": true}]
```

This second argument matters. Without it, authenticated and several public
methods fail with a generic server error.

The server invocation envelope is:

```json
{
  "success": true,
  "response": "<gzip bytes or object>",
  "status": 200,
  "msg": ""
}
```

With the JSON Hub protocol, `byte[]` is transported as a base64 string. The
web client base64-decodes it, ungzips it, and JSON-parses the result. A Kindle
client can either implement these steps or use MessagePack and preserve the
binary type.

The web client also receives server callbacks:

- `OnMessage`, `OnError`, `OnSuccess`
- `OnNotificationRefresh`
- `OnGrowthUpdate`
- `OnDirectMessage`
- `OnDirectMessageRead`
- `OnDirectMessageBlockChanged`

Reconnect behavior is 0s, 2s, 5s, 10s, then 15s for subsequent attempts.
After reconnecting, the client refreshes user state, shelf state and direct
message state because missed callbacks are not replayed.

### 1.3 Authentication

The SPA hashes the user's password with SHA-256 before sending it. Password
reset and registration follow the same rule.

HTTP endpoints:

| Method | Path | Request | Response |
|---|---|---|---|
| POST | `/api/user/login` | `{email, password: sha256}` | `{Token, RefreshToken}` |
| POST | `/api/user/register` | `{userName, email, password: sha256, code, inviteCode}` | `{Token, RefreshToken}` |
| POST | `/api/user/refresh_token` | `{token: refreshToken}` | access token string |
| GET | `/api/user/send_register_email` | `email` | empty |
| GET | `/api/user/send_reset_email` | `email` | empty |
| POST | `/api/user/reset_password` | `{email, code, newPassword: sha256}` | empty |

The access token in the SPA has an in-memory expiry of 30 seconds
(`VUE_SESSION_TOKEN_VALIDITY=30000`). The refresh token is longer-lived and is
used to obtain a new access token. A SignalR WebSocket commonly receives the
access token as an `access_token` query parameter. Sending both the query
parameter and `Authorization` header is harmless.

### 1.4 Hub method catalogue

All requests and responses use PascalCase fields unless noted.

#### Catalogue and search

| Hub method | Request | Response |
|---|---|---|
| `GetLatestBookList` | page, size, `IgnoreJapanese`, `IgnoreAI` | paged `BookInList` |
| `GetBookList` | page, size, `KeyWords?`, `Order`, filters, `CategoryId?` | paged `BookInList` |
| `GetBookListByTitle` | same as `GetBookList` | paged books |
| `GetBookListByAuthor` | same as `GetBookList` | paged books |
| `GetBookListByName` | same as `GetBookList` | paged books |
| `GetBookListByTags` | same as `GetBookList`, comma-separated tags | paged books |
| `GetBookCategories` | `Type: Novel|Comic` | category list |
| `GetSeriesList` | page, size, order, filters, category | paged series |
| `GetBooksBySeries` | series name plus list parameters | paged books |
| `GetBookListByIds` | `Ids`, optional `Type` | list of books/series |
| `GetRank` | `Days: 1|7|31` | ranked books |
| `GetComicList` | page, size, order | paged comic series |
| `SearchComicSeries` | keywords, mode, page, size, filters | paged comic series |

`BookInList` normally contains:

```text
Id, Type, SeriesTitle, Cover, LastUpdatedAt, UserName, Title,
Level, InteriorLevel, Category{ShortName,Name,Color}
```

#### Book details and reading

| Hub method | Request | Response |
|---|---|---|
| `GetBookInfo` | `{Id}` | `SeriesTitle`, `Series`, `Book`, `ReadPosition` |
| `GetNovelContent` | `{Bid, SortNum, Convert?}` | `{Chapter, ReadPosition}` |
| `SaveReadPosition` | `{Bid, Cid, XPath}` | empty |
| `GetReadPosition` | `{Id}` | read position |
| `GetReadHistory` | none | `{Novel: number[], Comic: number[]}` |
| `ClearReadHistory` | none | empty |
| `GetComicContent` | `{Cid, Skip, Take}` | chapter metadata and image URLs |

`GetNovelContent.Chapter` contains:

```text
BookId, BookName, Id, Content, Title, SortNum, CanEdit, Chapters, Font?
```

`Convert` is `t2s`, `s2t`, or null. `Content` is HTML and `Font` is a chapter
font URL or path. `ReadPosition` contains `ChapterId` and an XPath string.

Reading progress is not a scroll percentage. The web client turns the first
visible text element into a relative XPath such as `./p[3]` and stores it with
the chapter ID. This value is sent to `SaveReadPosition`, so a Kindle client
must generate a compatible relative XPath rather than a page number.

#### Shelf and user

| Hub method | Request | Response |
|---|---|---|
| `GetMyInfo` | none | current user, growth, unread counters |
| `GetUserSummary` | `{UserId}` | public user summary |
| `GetBookShelf` | none | `{data: ShelfItem[], ver}` |
| `SaveBookShelf` | `{data: ShelfItem[], ver}` | empty |
| `GetMyBooks` | type and list parameters | user books |
| `GetNotifications` | `{Page, Size}` | notification page |
| `MarkNotifications` | `{Ids}` | empty |
| `SignIn` | `{}` | reward, streak, growth |
| `GetPointLog` | `{Page, Size}` | point ledger page |
| `GetCoinLog` | `{Page, Size}` | coin ledger page |
| `GetSignInCalendar` | `{Year, Month}` | signed days |

Shelf item forms:

```json
{"type":"NOVEL","id":123,"index":0,"parents":[],"updateAt":"..."}
{"type":"FOLDER","id":"uuid","index":0,"parents":[],"title":"名称","updateAt":"..."}
```

`parents` is the complete path from root to the parent folder. `index` is the
position within its parent. The latest structure version in this source is
`20260921`.

#### Comments, shop and direct messages

| Hub method | Request |
|---|---|
| `GetComments` | `{Type: Book|Announcement, Id, Page}` |
| `PostComment` | `{Type, Id, Content}` |
| `ReplyComment` | `{Type, Id, Content, ReplyId, ParentId}` |
| `DeleteComment` | `{Id}` |
| `GetShop` | `{}` |
| `GetMyItems` | `{}` |
| `BuyShopItem` | `{Key, Quantity}` |
| `UseSignMakeupCard` | `{Date}` |
| `UseComicQuotaCard` | `{}` |
| `GetDirectConversations` | `{BeforeMessageId, Size}` |
| `GetDirectMessages` | `{PeerUserId, BeforeMessageId, Size}` |
| `SendDirectMessage` | `{RecipientUserId, ClientMessageId, Content}` |
| `MarkDirectMessagesRead` | `{PeerUserId, ThroughMessageId}` |
| `SetDirectMessageBlock` | `{UserId, IsBlocked}` |

There are also announcement methods:

```text
GetOnlineInfo
GetAnnouncementList({Page, Size})
GetAnnouncementDetail({Id})
GetCollaboratorList
GetBanList
```

## 2. Web novel rendering and font obfuscation

The normal web pipeline is:

1. Call `GetNovelContent({Bid, SortNum, Convert})`.
2. Sanitize `Chapter.Content` with DOMPurify.
3. Request `Chapter.Font` and inject:

```css
@font-face {
  font-family: read;
  font-display: block;
  src: url("<font-url-or-api-server-prefix>");
}
```

4. Render the sanitized HTML inside `.html-reader`.
5. Force `font-family: read, sans-serif !important` on the reader.
6. Wait for `document.fonts.ready`, then remeasure pagination and layout.

The important property is that the content string itself is not visibly
decoded by JavaScript. It is rendered with a chapter-specific font. This is a
glyph-mapping scheme:

- `Content` contains a stable sequence of characters/code points.
- The server font maps those code points to the intended visible glyphs
  through its `cmap`, glyph outlines and any OpenType substitution tables.
- Without the font, the same string can display unrelated characters.
- Applying a different font to the same string does not recover the intended
  appearance.

The mapping does not have to use private-use code points. In a live test of
book `20439`, chapter 2, 101 unique non-space characters were extracted, the
chapter font contained no PUA code points, yet rendering with the system font
produced visibly wrong characters. The chapter-font image differed from the
system-font image by approximately 7.94% of pixels. Therefore the correct
contract is "always render this content with this chapter font", not "decode
PUA into Unicode".

The web reader supports the following HTML/CSS subset:

- headings `h1` to `h4`, paragraphs, blockquotes and lists;
- inline bold, italic, colors and size classes;
- centered/right/left paragraphs;
- images, including `duokan-image-single`, `illus` and captions;
- Duokan footnotes (`a.duokan-footnote` and `.footnotes`);
- tables and simple alignment/float classes.

The site has two page modes:

- Continuous vertical scroll, optionally with tap-to-scroll.
- CSS multi-column flip pagination. CSS columns split the full chapter into
  screens; `scrollWidth`, column gap and viewport width determine the number of
  screens.

The Kindle implementation cannot use CSS. It parses the HTML into block
objects, measures each line with Pillow using the chapter font, and performs
the same wrapping and pagination directly.

### 2.1 Kindle font rendering

KinNovel handles the font as follows:

1. Download `Chapter.Font` into a content-addressed cache.
2. Use Pillow's FreeType binding to load it.
3. Use the *same font object* for both width measurement and drawing.
4. If the server returns WOFF1, decompress its zlib table records and rebuild
   an SFNT TTF/OTF container before loading it in Pillow.
5. Use the system CJK font only for UI labels and as an explicit fallback.

The bundled Kindle Pillow 12.3 can load the live WOFF2 chapter font when the
KOReader FreeType library is preloaded; this was verified on the target device.
If a different Pillow build lacks WOFF2/Brotli support, the reader falls back
to the system font instead of inventing a text transformation.

The currently deployed official web reader does not implement per-character
fallback in JavaScript. It injects:

```css
@font-face {
  font-family: read;
  font-display: block;
  src: url("<chapter-font>");
}
```

and uses:

```css
font-family: read, sans-serif !important;
```

There is no `unicode-range` on the chapter font, so the browser's normal CSS
font fallback selects `sans-serif` when `read` has no glyph for a code point.

A live inspection of book `9990`, chapter 5 found 12,701 chapter characters and
1,385 unique non-space code points. Seven code points were absent from the
chapter font and all seven were available in the Kindle system font:

```text
U+200B, U+200C, U+200D, U+2500, U+25A0, U+25BC, U+FEFF
```

Pillow does not automatically apply CSS-style font fallback when it is given a
single `FreeTypeFont`. KinNovel therefore performs the equivalent operation
explicitly: it checks each character's glyph mask and uses the system font only
for characters whose chapter-font glyph is absent or empty.

The reader generates relative XPaths from the parsed block DOM and sends the
first visible block's path to `SaveReadPosition`. This keeps progress compatible
with the web client.

## 3. kComics Kindle implementation

kComics is a GPLv3 Python application. Its core is not a browser and does not
use Qt or a GUI toolkit. It directly owns the e-ink framebuffer and the touch
event device.

### 3.1 Installation and process lifecycle

- `config.xml`, `menu.json` and `manifest.json` expose the app to KUAL/KPM.
- `start.sh` detects processes holding `/dev/fb0`, sends them `SIGSTOP`, and
  records their PIDs.
- It saves the current framebuffer to a binary snapshot.
- It exports `LD_LIBRARY_PATH` for bundled ARM libraries and launches Python.
- On exit, it restores the snapshot, sends `SIGCONT`, and asks `appmgrd` to
  bring the system home screen back.

This is the key compatibility technique for a foreground framebuffer app:
Kindle's own programs do not need to be terminated; they are paused and then
resumed.

### 3.2 Framebuffer and EPDC output

`EInkDisplay` opens `/dev/fb0`, reads `FBIOGET_VSCREENINFO` and
`FBIOGET_FSCREENINFO`, maps the framebuffer with `mmap`, and submits refresh
regions through EPDC ioctls.

Two hardware protocols are implemented:

- `mxcfb`: standard i.MX EPDC. Update structures use
  `MXCFB_SEND_UPDATE = _IOW('F', 0x46)`, waveform constants `DU`, `GC16`,
  `A2`, `GL16`, `AUTO`, `REAGL`, and related flags.
- `mtk`: MediaTek hwtcon used by some Paperwhite devices. It uses update
  command `0x2E`, wait command `0x2F`, and a 96-byte update structure with
  different waveform values.

Pillow creates 8-bit `L` images. `write_image` copies grayscale bytes into the
mmap at the correct line stride. `show` chooses:

- partial refresh for ordinary page state changes;
- full flashing refresh for menus/dialogs when required;
- waveform upgrades for `is_flashing`, dithering and REAGL devices;
- region-based refresh to avoid flashing the entire screen.

A mutex serializes display updates because download/export worker threads and
the input thread can both request refresh.

### 3.3 Touch input

The input layer uses `evdev`:

- It scans `/dev/input/event*` and identifies devices with multi-touch
  position axes (`ABS_MT_POSITION_X/Y`, slots and tracking IDs).
- Touch hardware coordinates are normalized and mapped to the render
  resolution, so the same page code works on different Kindle display sizes.
- `MultiTouchParser` converts kernel events into `tap`, `long`, `up`, `down`,
  `left` and `right` dictionaries containing pixel and ratio coordinates.
- The app grabs the touch device so the system launcher does not also react.

### 3.4 UI and threading

kComics has a page registry:

```python
PAGES = {
  "home": (home.render, home.handle),
  "search": (search.render, search.handle),
  ...
}
```

Every page renders a complete Pillow image. Layout constants are ratios of the
current screen dimensions, then converted to pixels with `px`/`rect`. This
makes one implementation usable across different Kindle models.

Network operations, image downloads, search and export run in daemon threads
or `ThreadPoolExecutor` workers. They update page state and call a locked
`_show` function when new state is ready. Download progress is throttled to
roughly 0.3 seconds to avoid excessive e-ink refreshes. A heartbeat thread
repaints a small activity indicator during long image or MOBI work.

### 3.5 Network and content

The kComics API client:

- uses `urllib.request` with explicit API headers;
- adds `Token` authorization only where required;
- retries transient network errors;
- lowers to an unverified TLS context only after certificate verification
  fails;
- caches comic detail responses in memory;
- downloads images to temporary files and atomically renames them;
- converts WebP/JPEG pages to progressive book assets when exporting.

Its source-sorting and image processing model is useful for a Kindle novel
client, but its MOBI/Calibre export and comic page pipeline are not reused by
KinNovel because download/export is explicitly out of scope.

## 4. Portability conclusions

The viable Kindle stack is:

- direct framebuffer + EPDC refresh from kComics;
- evdev touch and the pinyin keyboard from kComics;
- a pure Python SignalR/WebSocket client for LightNovelShelf;
- Pillow-based HTML subset rendering and manual pagination;
- chapter-specific fonts used for measurement and drawing;
- local file cache and an offline-first shelf;
- no browser, no Qt, no Calibre, and no manga image pipeline.

The main compatibility risks are Pillow builds without WOFF2 support,
device-specific EPDC protocols, TLS certificates on old firmware, large
embedded illustrations, and SignalR server-side changes outside the public web
client.
