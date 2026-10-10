# KinNovel 1.0 — UI design

The Rust rewrite gets a new interface (decided by the user: layouts are redesigned, not ported). Goals, in order:
**operable on e-ink** (big targets, no scrolling, few full refreshes) → **fast to the text** (one tap to resume reading) → **calm and legible** (black on white, generous whitespace, one accent: solid black).

Baseline: KPW5, 1236 × 1648 px, 300 ppi. All sizes below are at that baseline and scale with `Metrics::for_screen` (0.75–1.15).

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
  - **书架** — "继续阅读" card (last book/chapter/page, one tap → reader at the saved position), then the user's shelf (cloud shelf when logged in, offline cache otherwise).
  - **历史** — reading history (cloud + local), newest first.
  - **发现** — segmented control: 最新 / 排行 / 分类; search is not offered (no keyboard).
  - **我的** — account card (login state, sign-in), settings, notifications, announcements, about, exit.
- Pushed pages (book detail, catalog, series, settings sub-pages, reader) show a back arrow in the header and **no tab bar**.
- The reader is full-screen; its overlay menu has back + home (home = shelf).

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
| List page change / tab switch | `Ui` (GC16, diff rect) | mostly full screen, needs clean greys |
| Press feedback | runtime (invert + A2) | instant |
| Toast show/hide, overlay menu | `Ui` | small rect |
| Night mode toggle, wake from sleep | `Flash` | full inversion |

## Text and data

- Lists show at most one line per field; long titles end with "…".
- Numbers: "第 3 章", "12 / 87 页", times as "3 分钟前 / 昨天 / 10-08".
- Error copy in Chinese, short, with a next step ("网络不可用，点击重试").
