# KinNovel — UI design

The Rust implementation (1.x) has its own interface (decided by the user: layouts are redesigned, not ported). Goals, in order:
**operable on e-ink** (big targets, no scrolling, few full refreshes) → **fast to the text** (one tap to resume reading) → **calm and legible** (black on white, generous whitespace, one accent: solid black).

Baseline: KPW5, 1236 × 1648 px, 300 ppi. All sizes below are at that baseline and scale with `Metrics::for_screen` (0.45–1.15, so 600 × 800 and 758 × 1024 screens fit). Reader font size and margins scale by ppi instead.

## Tokens (`kn_ui::widgets::Metrics`, `kn_ui::Theme`)

| Token | px | Use |
|---|---|---|
| `hero` | 90 | app name on the about page only |
| `title` | 50 | header titles, dialog titles |
| `body` | 40 | list titles, buttons in dialogs, body UI text |
| `small` | 33 | subtitles, buttons, meta |
| `tiny` | 27 | reader header, status text, captions |
| `touch` | 106 (≈ 9 mm) | minimum height/width of anything tappable |
| `margin` | 48 | page side margin; gaps are `margin / 2` |
| `radius` | 12 | buttons, cards, dialogs |

Colours: day = white bg / black fg, `muted` 105 for secondary text, `mid` 170 for hairlines, `light` 225 for header bars. Night = inverted (`Theme::new(true)`). Never use grey > 200 to carry information (invisible on e-ink).

## Navigation

```
┌──────────────────────────────┐
│ Header: title · status       │  ← back arrow only on pushed pages
├──────────────────────────────┤
│                              │
│  page content (paged, no     │
│  scrolling: swipe up/down or │
│  pager buttons change page)  │
│                              │
├──────────────────────────────┤
│ 书架 │ 历史 │ 发现 │ 我的    │  ← tab bar, top-level pages only
└──────────────────────────────┘
```

- The app opens on **书架** (shelf). There is no menu grid any more.
- Four top-level tabs; switching tabs replaces the stack root (`Transition::Home`-like), never deepens it.
  - **书架** — "继续阅读" card (cover on the left — the catalog cover URL remembered with the last-read record, else the first image of the cached first chapter; last book/chapter/page; one tap → reader at the saved position), then the user's shelf (cloud shelf when logged in, offline cache otherwise).
    Cloud folders show as rows with a folder icon; tapping one opens it in place (header gets a back arrow to the parent level, the stack does not grow).
    Long press a book → confirm "移出书架"; long press a folder → confirm delete (its books move up one level). Every write fetches the shelf fresh,
    changes one entry and saves the whole table (`SaveBookShelf` overwrites); the result shows as a toast.
  - **历史** — reading history (cloud + local), newest first; segmented 小说 / 漫画 (comic history is aggregated by series, like the web client).
  - **发现** — segmented control: 最新 / 排行 / 分类 / 漫画 (second row: 日榜/周榜/月榜 for 排行, 最近更新/上架时间/总点击量 for 漫画); a magnifier in the header opens **搜索**: 小说 / 漫画 tabs, modes 模糊 / 精确 / 书名 / 作者 / 系列 / 标签, recent searches, and the on-screen keyboard (en + pinyin, `kn_ui::keyboard` + `kn-ime`). The input field sits right above the keyboard so a keystroke refreshes only the lower half.
  - **我的** — account card (logged out: 登录 button; logged in: name/level/coins, 每日签到 + 退出登录 with a confirm dialog) and the entry list below.
- **Welcome / login** (`pages/login.rs`) — the home page on an online start while nobody is logged in and 「先逛逛」 was never tapped (config `welcome_skipped`); also pushed from 我的 and from every 「需要登录」 state (history, comments, notifications, shop) as a 登录 button. Fixed layout: title, one-line intro, 邮箱 / 密码 fields (password masked with ●, 显示/隐藏 toggle), one message line, full-width 登录. Tapping a field opens the keyboard (`TextEditor::render_keys`) over the lower half, where 先逛逛 and the registration note (register on the website) sit otherwise; Enter = 下一项 / 登录, tapping above the keyboard closes it. Both fields start in English mode.
- Book detail: cover, meta, intro, action row 开始/继续阅读 · 目录 · 加入书架 / 移出书架 (logged in) · 评论; author and each tag start a search.
- Pushed pages (book detail, catalog, series, settings sub-pages, reader) show a back arrow in the header and **no tab bar**.
- The reader is full-screen; its overlay menu has back + home (home = shelf).
  Under the menu header (novel and comic reader alike) a quick-light panel: rows 亮度 / 色温 = value · − · slider · ＋
  (tap the slider to jump, swipe on it to drag; only the panel region refreshes, `Ui`). Values are powerd's own
  (`flIntensity` 0..flMaxIntensity, `currentAmberLevel` 0..24 via `lipc-set-prop`), so they persist after exit;
  rows the device doesn't support are hidden (no panel at all without a frontlight).
- Novel reader: taps on the thin status band at the top (chapter title + page on the left, time · battery right-aligned like every other header) do nothing (no accidental turns).
  Menu header = back · book title · bookmark ribbon (outline / filled = toggle a bookmark on this page) · home.
  The chapter progress bar in the bottom panel is a slider (tap or swipe → page in the chapter, whole chapter laid out first).
  Illustrations: only the cover (first image of the volume's first chapter, with no body text before it) fills a page;
  every other image is laid out with its real size (the CDN URL carries `size=WxH`), at most 62 % of the text area high,
  and flows with the text. Images narrower than half the text width (gaiji standing in for rare characters) keep their natural size.
  Body typography follows the web reader (`kn_text::style`, from LightNovelShelf/Web `src/css/read.scss` plus browser defaults):
  bold (synthetic: glyph drawn 1–2 px wider, advances unchanged), italic (sheared outline), per-run font size, sup/sub,
  underline / strike-through, emphasis dots, ruby above the base, centre/right alignment, `<p>`-only first-line indent,
  heading sizes and margins, blockquote (inset, grey text, left bar), `<hr>` rule, list markers, inline gaiji images.
  The reader's own settings (font size, line spacing, paragraph spacing, indent switch) still apply; colours, floats and borders are ignored.
  Footnotes (`.duokan-footnote`, `epub:type=noteref`, markdown-it refs): the note body is hidden like on the web and the marker
  is a small boxed 「注」; tapping it (hit area widened by half a line on each side) opens a centred card titled 注释 with the note text
  in the chapter font; any tap closes it (`Clean` region flash). Block paths and code-point offsets are identical to the plain
  layout, so reading positions stay compatible.
  A bookmarked page shows a small filled ribbon hanging from the top edge in the right margin.
  目录 from the reader = "目录与书签": segmented 目录 / 书签 (n); bookmark rows = chapter title, excerpt (drawn with the
  chapter's obfuscation font, which is remembered per bookmark), relative time; long press → delete dialog.
  Picking a chapter or bookmark returns to the same reader instance, which reloads/jumps in place (`ReaderJump`), so the stack does not grow.
  Bookmarks live in `data/bookmarks/{book}.json` (not under `cache/`, so cache clearing keeps them).
- 我的 entries: 消息通知 / 积分商城 (logged in) / 公告 / 阅读统计 / 设置 / 存储空间 / 关于 / 退出 (rows `touch × 1.12` so 8 fit).
  阅读统计 = three cards (今天 / 近 7 天 / 累计: time + pages), streak line, 14-day bar chart (today black), top 5 books.
  存储空间 = total + per-kind rows (章节 / 章节字体 / 插图与封面 / 漫画) with 清理, 全部清理; confirm dialog each time.
  关于 shows 有新版本 x.y.z when the daily GitHub check found one; the dialog has 检查更新.
- Sleep screen (设置 → 显示 → 休眠显示封面, restart needed): cover of the last-read book fitted above an info band
  (book title, chapter, "KinNovel · 休眠中"); without a cached cover a bordered title card. Full GC16 flash.
- Comics (`Book.Type == "Comic"`) use the same detail page ("漫画详情", catalog rows show page counts) and open the **comic reader**:
  one page fitted to the whole screen, no bars. Tap left/right 30 % = previous/next (mirrored when 翻页方向 = 从右往左),
  middle = overlay menu (话名 · 页码, progress bar filling from the reading side, 上一话 / 目录 / 放大 / 下一话,
  翻页方向 / 每页全刷 / 设置). Long press = zoom (the illustration preview page). Settings has a 漫画 group.

## Components (`kn_ui::widgets`)

All components draw immediately into the frame and register hits; they never keep state.

- **header** `(title, status, back)` — `touch × 1.1` high, `light` fill, hairline below. Back = left square hit; status (time · battery) right-aligned `tiny`.
- **tab_bar** `(tabs, active)` — bottom, `touch × 1.15` high, hairline above; active tab: black fill + white label; others: label only. No icons (font glyph coverage is unreliable).
- **list_row** `(title, subtitle, meta, cover?)` — height `touch × 1.35` (`× 1.9` with cover). Title `body`, one line, ellipsis; subtitle `small muted`; meta right-aligned `small muted`; hairline separator inset by `margin`. Whole row is the hit target with press feedback.
- **pager** `(page, pages)` — footer `touch` high: `‹ 上一页` · `n / m` · `下一页 ›` (text buttons, disabled at the ends). Swipe up = next page, swipe down = previous page. Page size = rows that fit; never partial rows.
- **segmented** `(options, active)` — row of equal buttons, active = Primary, others = Secondary.
- **card** — rounded rect, 2 px `foreground` border, `margin/2` padding. Used for "继续阅读" and the account card.
- **button** — `Primary` (black fill) for the single main action of a screen, `Secondary` (outlined) for the rest, `Disabled` greyed.
- **dialog** `(title, lines, buttons)` — modal card centred at 80 % width, 3 px border, white fill, buttons in a row at the bottom; taps outside are ignored (no accidental dismiss). Used for confirmations and errors.
- **toast** `(text)` — inverted pill above the bottom edge, auto-hides after 2 s (needs a timer in `kn-ui`); never used for errors that need action.
- **states** — `loading` ("加载中…" centred, muted), `empty` (centred muted text + optional action button), `error` (message + 重试 Primary button).

## Refresh policy per interaction

| Interaction | Hint | Why |
|---|---|---|
| Page turn in reader | `Turn` (REAGL full screen) | text everywhere, ghosting handled by REAGL + budget |
| Page turn in comic reader | `Flash` + swipe animation (`request_flash_turn`, default `comic_page_flash`) or `Turn` + animation | large grey areas ghost badly under partial refresh; a page whose image is still decoding shows a note (`Ui`) and flashes (animated) when the image arrives; a prefetched next chapter opens in the same frame |
| List page change / tab switch | `Ui` (GC16, up to 4 diff regions) + region flash where a solid dark block (old selected tab/chip, inverted row) turns back to background | partial refresh leaves dark blocks as ghosts; text-only changes never flash |
| Press feedback | runtime (invert + A2 for small targets such as keys, DU for larger ones) | instant; KOReader uses `a2` only on the keyboard, `fast` elsewhere |
| Toast show/hide, overlay menu | `Ui` | small rect |
| Night mode toggle, wake from sleep | `Flash` | full inversion |

## Text and data

- Lists show at most one line per field; long titles end with "…".
- Numbers: "第 3 章", "12 / 87 页", times as "3 分钟前 / 昨天 / 10-08".
- No permanent tutorial text in page layouts. Gestures are explained once per page in a tutorial dialog (`pages/tips.rs`: shelf, reader, comic reader, keyboard, catalog with bookmarks, image viewer) with two buttons: 「感觉会忘记」 (show again after the next launch) and 「我知道了」 (never again, stored in config `tips_seen`). Settings → 显示 → 操作提示 turns them off; turning it back on clears `tips_seen`. The README's 使用 section lists the same gestures.
- Error copy in Chinese, short, with a next step ("网络不可用，点击重试").
