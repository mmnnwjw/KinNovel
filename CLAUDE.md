# KinNovel — guide for Claude

Kindle (jailbroken, KUAL) client for LightNovelShelf (轻书架). Two implementations:

| Line | Where | Status | Version |
|---|---|---|---|
| Python | branch `legacy` only (`bin/src`, `tests/`, Python tools) | Released, maintenance only | 0.8.x |
| Rust | `main` (from v1.0.0): `rust/` + KUAL files at the repo root (`bin/kinnovel.sh`, `bin/config.json`, `menu.json`, ...) | Current | 1.x |

The Python sources are no longer on `main`. For porting questions read them from `legacy` (`git show legacy:bin/src/kinnovel/pages/reader.py`, or `git worktree add ../kn-legacy legacy`).

Versioning rule (from the user): Python work is 0.8.x; **only the Rust implementation becomes 1.0.0**.

UI design rule (from the user): in the Rust rewrite, all screen layouts and visual design are **ours to decide afresh** — don't copy the Python layouts; optimise for looks *and* Kindle operability (large touch targets, e-ink-friendly contrast, few full refreshes). Behaviour that must stay compatible: reading-position anchors (XPath + code-point offset), cache/config formats, API usage.

## Python line (0.8.x, branch `legacy`)

- Run tests (on `legacy`): `PYTHONPATH=bin/src python -m unittest discover -s tests` (286 tests, ~17 s). Must stay green.
- Credential scan before any release: `python tools/credential_scan.py`.
- Release zip (legacy): `python tools/package_release.py` → `build/KinNovel-vX.Y.Z.zip` (check contents: no credentials/cache/tests).
- Version strings to bump together (legacy): `bin/src/kinnovel/__init__.py` (VERSION, BUILD), `config.xml`, `manifest.json`, `README.md` links.
- Architecture: `app.py` → `PageContext` (ui.py) → page modules in `pages/` with module-level `STATE`. Display refresh pipeline (diff-region refresh, ghost budget, REAGL turns, idle prerender) is in `ui.py` `PageContext.show/_refresh_plan`. History of reviews and measured numbers: `docs/code-review.md`.
- Commit message style: Conventional-ish with Chinese summary, e.g. `perf(release): v0.8.0 ...`, `feat(rust): ...`.

## Rust line (`rust/`)

Read `rust/DESIGN.md` first (goals, crate layout, Phase 0 measurements), then `rust/research-kindle-devices.md` (per-model quirks, FBInk waveform table, licensing).

Workspace crates (dependency direction app → ui → render/platform):
- `kn-render` — L8 `Bitmap` (stride-aware: KPW5 stride 1248 ≠ width 1236), geometry, fonts (`woff2-patched` + `skrifa` + `zeno`, pure Rust, no FreeType), glyph cache, text.
- `kn-platform` — `FbinkDisplay` (FBInk statically linked, Linux target only), `MemoryDisplay` (host), gesture recognizer (port of `bin/src/screen/input/parser.py`), evdev `InputReader` (`fbink_input_scan`), `PowerMonitor` (`lipc-wait-event` + watchdog).
- `kn-ui` — immediate-mode pages + `RefreshScheduler` (diff + waveform + ghost budget, ported from Python 0.8.0), `Hits`, worker pool whose results are routed to the **page instance** that spawned them (`Page::on_message`) — page state lives in page structs, no globals. `run()` is a single `poll()` loop (input/power/eventfd), sleeps until the next minute; press feedback = invert + A2.
- `kn-text` — HTML → blocks (html5ever), kinsoku line breaking, progressive `Paginator`, anchors. Must match the Python layout exactly: golden files from `tools/kn_text_golden.py` (deterministic widths); synthetic fixtures are committed, real chapters live in the gitignored `tests/fixtures/private/` (copyrighted — never commit; refresh from the device cache `content/*.json`).
- `kn-app` — binary `kinnovel`: `store.rs` (Python-compatible config/chapter/progress/font cache; font WOFF2 → `.ttf` sidecar), `pages/` (home, library = cached chapters, reader). Host preview drives the real pages via `kn_ui::Headless`: `KN_APP_DIR=<copy of device app dir> kinnovel --preview out.pgm [--read BOOK SORT] [--tap X,Y]... [--swipe left|right|up|down]...` (build it with `cargo-zigbuild zigbuild --release --target x86_64-pc-windows-gnu -p kn-app`).
- Spikes (`rust/spike-*`) are excluded from the workspace; reference only.

Build / test (Git Bash on Windows; host toolchain is `x86_64-pc-windows-gnu`, no MSVC):
```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd rust && ./build.sh host-test            # host tests (cargo-zigbuild test --target x86_64-pc-windows-gnu --workspace)
./build.sh kindle                          # armv7-unknown-linux-musleabihf static binary via cargo zigbuild
./build.sh host                            # host preview binary (kinnovel.exe --preview)
```
- Release (Rust): versions in `rust/Cargo.toml` (workspace), `config.xml`, `manifest.json` must match — `python tools/package_release.py` checks them, builds the device binary, refuses credentials, writes `build/KinNovel-vX.Y.Z.zip` (KUAL layout under `KinNovel/`). Run `python tools/credential_scan.py` first.
- Host previews with synthetic online data: `KN_FAKE_DIR=rust/crates/kn-app/tests/fixtures/fake` (every `api::load_*` reads `<name>.json` instead of the network).
- Plain `cargo test` fails on this host: html5ever → parking_lot → windows-link needs `dlltool`/`as` (no mingw binutils installed). Always use `./build.sh host-test` (extra args are passed to cargo test).
- `rust/.cargo/config.toml` sets `-C target-feature=+neon` for the device (rustc prints an "unstable feature" warning — expected). Don't switch to `target-cpu=cortex-a8`: measured 2× slower text rendering (LLVM's A8 cost model stops vectorizing pixel loops) and it doesn't expose `cfg(target_feature="neon")`.
- Perf numbers: `kn-render/examples/bench` (medians of 15) — build with `cargo zigbuild --release --target armv7-unknown-linux-musleabihf -p kn-render --example bench`, run from `/tmp` on the device. CPU governor is `ondemand` (600–1000 MHz): expect ±1 ms noise, compare runs back-to-back.
- `build.sh` sets `CARGO_ZIGBUILD_ZIG_PATH` to the zig from the `ziglang` pip package.
- Crates needing C on the **host** (e.g. `ring` later) must be built with `cargo zigbuild --target x86_64-pc-windows-gnu`.
- FBInk is a git submodule at `rust/third_party/FBInk`, pinned to the device-validated commit; run `git submodule update --init rust/third_party/FBInk` after cloning. Don't recurse into FBInk's own submodules (not needed for the MINIMAL+INPUT build).

## Device testing (real Kindle)

- Test device: KPW5 (MT8110 Bellatrix, FW 5.17.1, armv7 hard-float, glibc 2.20) at `root@192.168.1.6:2222`, empty password. OpenSSH BatchMode fails; use paramiko (no SFTP subsystem — upload via `exec_command("cat > path")`).
- Secrets (GitHub token, LightNovelShelf test account) are in `C:\Users\mmnnwjw\.kinovel-secrets\`. Never print them, never commit them, never copy them into the repo.
- Installed app: `/mnt/us/extensions/kinnovel` (Rust 1.0.0 since 2026-10-10, installed from the release zip via `install.sh`; the previous Python 0.8.0 install minus `cache/` is backed up at `/mnt/us/kinnovel-0.8-backup.tar.gz`). Don't modify it unless asked.
- `/mnt/us` is vfat (no exec bit): copy binaries to `/tmp`, `chmod +x` there, delete afterwards.
- Testing the UI without a human: inject touches by writing `struct input_event` (16 bytes, 32-bit timeval) to `/dev/input/event1` with the device's Python (`/mnt/us/python3/bin/python3.14`): MT type B — `ABS_MT_SLOT 0, ABS_MT_TRACKING_ID n, ABS_MT_POSITION_X/Y, BTN_TOUCH 1, SYN`, wait ~80 ms, `TRACKING_ID -1, BTN_TOUCH 0, SYN`. KPW5 touch range = screen pixels (0..1235, 0..1647). Read the result back with `dd if=/dev/fb0 bs=1248 count=1648` (8-bit, stride 1248) and crop to 1236 wide. Deploy to `/tmp/kn_demo/bin/{kinnovel,kinnovel.sh}`, copy the app data to `/tmp/kn_app` (`bin/config.json` + `cache/`) and start with `KN_APP_DIR=/tmp/kn_app KN_DEBUG=1 nohup sh kinnovel.sh` — the installed app's data stays untouched. `KN_DEBUG=1` logs every input event and per-frame render/plan/present times. Wait ~12 s after launch before the first tap (launcher scans /proc). BusyBox `sleep` takes integers only. Exit via the 退出 button or `kill -TERM $(pidof kinnovel-bin)`; afterwards `rm -rf /tmp/kn_demo /tmp/kn_app` (the config copy contains the account login). Don't `cat` the config — it holds credentials; read single keys.
- Taking over the screen: pause processes holding `/dev/fb0` (SIGSTOP), write their PIDs to `/tmp/kinnovel_paused_pids`, and on exit always SIGCONT them and run `lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home` (see `bin/kinnovel.sh`). Afterwards verify no process is left in state `T`.
- **Rate limits:** the LightNovelShelf test account can be banned. Keep live API calls to a handful, ≥ 6 s apart; prefer cached data on the device (`/mnt/us/extensions/kinnovel/cache/`) and unauthenticated endpoints (`GetLatestBookList` is public; `GetBookList` needs auth).

## Windows shell gotchas

- Git Bash rewrites `/tmp/...` arguments into Windows paths → `export MSYS_NO_PATHCONV=1` when passing device paths, and then give local paths in `E:/...` form.
- Python `open(p, 'w')` writes CRLF; repo files are LF → use `newline=''` (or binary) in edit scripts. Write edit scripts to files rather than heredoc'd Python with escapes (a `\\0` once became a literal NUL byte).
- The session's working directory inside a folder makes Windows refuse to delete that folder.

## Working conventions

- Measure on the device before claiming a performance win (cProfile on the host misled once: a "1.5× faster" line-wrap rewrite had zero wall-clock gain on the Kindle and was reverted).
- Commit locally on the working branch; push / release / tag only when the user asks.
- End commit messages with the `Co-Authored-By` line given in the session's attribution reminder.
- No global git identity on this machine: commit with `git -c user.name=mmnnwjw -c user.email=mmnnwjw@users.noreply.github.com commit ...` (same as existing history).

## Subagents (main agent: Opus 5.5)

Pick the subagent model by task and always set it explicitly (`model:` in `.claude/agents/*.md`, or the Agent tool's `model` parameter); don't rely on the default. All subagents use the **Claude 5.5 family** — Haiku 5.5 (`claude-haiku-5-5`), Sonnet 5.5 (`claude-sonnet-5-5`), Opus 5.5 (`claude-opus-5-5`); the short aliases `haiku` / `sonnet` / `opus` resolve to these. Don't use older generations.

| Task | Model | Examples in this repo |
|---|---|---|
| Read / locate / summarize code | Haiku 5.5 (`haiku`) | Find where a Python behaviour lives in `bin/src/` before porting; list call sites in a crate; summarize `docs/code-review.md` sections |
| Read for porting semantics | Sonnet 5.5 (`sonnet`) | Compare `reader.py` layout or `ui.py` refresh logic against the Rust port and report behavioural differences |
| Implement a well-specified change | Sonnet 5.5 (`sonnet`) | Port a page module to `kn-ui`; `kn-store` readers for Python's config/session/progress/cache formats; add host tests |
| Hard or high-risk code | Opus 5.5 (`opus` / `inherit`) | `RefreshScheduler` / waveform / ghost-budget logic, FBInk FFI and `unsafe`, `kn-render` perf work (`blend_mask`, `diff_bbox`), worker-pool message routing |

Keep these on the main agent, never delegate:
- Anything on the Kindle (SSH/paramiko, screen takeover, SIGSTOP/SIGCONT, `/tmp` binaries) — a subagent that exits early can leave processes paused in state `T`.
- Live LightNovelShelf API calls (rate limits / account ban risk).
- Releases, version bumps, credential scan, commits, push/tag.
- Final review of subagent output and the decision that a perf change is a win (device measurement required).

Subagents get only their own prompt, not this file's context. When delegating, state explicitly:
- Branch (`main` for Rust vs `legacy` for Python) and which line (Python 0.8.x / Rust) the work belongs to.
- The scope boundary (files/crates it may touch) and the expected return format (paths + line numbers, or a diff summary).
- The test command to run and that it must stay green: `cd rust && cargo test --workspace` or the Python `unittest` command above.
- Never run whole-disk searches (`find /`, `find / -iname ...`) — they take 10+ minutes on this Windows machine and have twice been left running after the subagent returned. Search only inside the repo, `~/.cargo/registry`, or `~/.rustup`, and make sure no background command is still running before returning.
- Repo rules: LF line endings (`newline=''`), `export PATH="$HOME/.cargo/bin:$PATH"`, `MSYS_NO_PATHCONV=1` for device-style paths, no secrets read or printed, no commits.

## Where the Rust line stands

- **v1.1.0 released** (2026-10-10, tag `v1.1.0`, installed on the KPW5). v1.0.0 was released earlier the same day. All pages: tabs 书架/历史/发现/我的; book detail, catalog, series, comments; announcements, notifications, points shop; settings page (阅读/显示/内容); reader with network (chapters/fonts/illustrations on demand, progress upload, optional neighbour prefetch); startup cache pruning (`cache_limit_mb`, LRU by mtime).
- UI: `rust/UI-DESIGN.md` is the spec for every page.
- Network degradation (2026-10-10): `kn-net` `ServerHealth` circuit breaker (network/timeout/502–504 → down, probe after 30 s doubling to 5 min); chapters are stale-while-revalidate (cached chapter opens in ~70 ms with the server down, was 3–7 s, once 30 s); progress upload / prefetch / cloud shelf are skipped while down. MTK page-turn swipe animation restored (`page_turn_animation`, device-verified). WebP images supported (the CDN serves WebP under `.jpg` URLs).
- Refresh policy outside the reader (2026-10-10, after user ghosting reports): Push/Replace/Back/Home flash the full screen (a page can opt out with `Page::flash_on_enter`; the reader does and flashes itself once the chapter is laid out); theme change → flash (run loop compares `App::theme()`); `RefreshHint::Clean` = region flash, used when a dialog closes; settings group switch flashes; UI ghost threshold is min(`full_refresh_every`, 3) screens (page turns keep `full_refresh_every`).
- Reader extras (2026-10-10): header shows chapter · time/battery (centre) · page; full-page illustrations (layout `image_max_ratio` 1.0 and `unknown_image_full`; golden tests keep Python's 0.62 / width×0.72); tapping an illustration (its drawn rect only) opens `pages/image.rs` — fit/zoom ×1.5–4 (bilinear `Bitmap::sample_into`), swipe to pan half a screen, 原图 downloads the URL without the CDN `height=` (cached under `Paths::original_image_file`). Touch seed after restart uses `EVIOCGMTSLOTS` (EVIOCGABS value is 0 for MT axes on KPW5).
- Comics (2026-10-10, unreleased, built from the web client `LightNovelShelf/Web` `services/manga` + `pages/Manga/Reader.vue` while the API was down): `GetComicList`, `GetComicContent {Cid, Skip, Take=6}` (URL batches; `ReadPosition` only when Skip=0), progress via `SaveReadPosition` with XPath = 1-based page. `kn-app/src/comic.rs` = cache (`cache/comic/{chapters,books,pages}`, own quota `comic_cache_mb`) + `progress/comic-<bid>.json`; `pages/comic.rs` = reader. Shelf keeps COMIC items (`GetBookListByIds` without Type); history comic tab uses `Type: Comic` (series-aggregated). Fixtures: `python tools/gen_comic_fixtures.py` (synthetic PNG pages, `fixture:` URLs resolved only in KN_FAKE_DIR mode). Device-verified with fixtures (KPW5): open/resume, prefetch (turn = render ~10 ms + flash), spread page, RTL, zoom, next chapter, continue card. **Not verified against the live API** — field names follow the web client source.
- KPW5 measurements: page turn = render ~4 ms + present 6.5–9 ms REAGL (Python 0.8.0: 41 ms); chapter open ~365 ms bg on first font decode (then cached as `.ttf`) + 56 ms layout; illustration decode+fit ~100–130 ms bg.
- Device-verified (offline, KN_OFFLINE=1 and online-with-server-down): shelf/local cache, continue-reading card, reader resume/turns/menu/illustrations, settings incl. night mode, sleep/wake (power key injected on event0 + `powerd_test -p`), clean exit (no stopped processes). Online mode with the server returning 502: shelf falls back to the local cache, lists show an error + 重试.
- Open items:
  - **Live API not yet verified**: api.lightnovel.life returned 502 for every request on 2026-10-10 (server outage). Online pages were tested only against synthetic fixtures (`KN_FAKE_DIR`). When it's back: `cargo run -p kn-net --example live` (one anonymous call), then one logged-in session on the device; compare `kn-app/src/api.rs` parsing with real responses (shop `Items`/`Coin`, comments `Users`/`Commentaries`, notifications, announcements are the least certain).
  - Not ported from 0.8: shelf write operations (add/remove book, folders), shelf folder navigation, `home_order`, `prefetch_reading_target`.
  - Physical power button check (key injection can't reach powerd).
  - Taps in the reader's header band turn pages (Python ignored taps above the content).
  - WebP decode is ~590 ms on KPW5 for a 1045x1536 illustration (JPEG ~100–130 ms); possible win: decode only luma from VP8 instead of RGB → gray.
