# Rust rewrite — Phase 0 spike guide (shared by all spike tasks)

Goal of Phase 0: prove each risky piece of the native stack works **on the real Kindle**
before the rewrite starts. Each spike is a small standalone cargo crate under `rust/spike-*`.

## Target device
- Kindle Paperwhite 5 (MT8110 "Bellatrix"), FW 5.17.1, kernel 4.9.77, **armv7 hard-float**,
  glibc 2.20 (too old to rely on) → we build **fully static musl** binaries.
- Screen 1236x1648, 8bpp grayscale, `line_length=1248`, EPDC protocol = MTK hwtcon.
- Touch: `/dev/input/event1` (pt_mt, multitouch), power button `/dev/input/event0`.
- ~190 MB RAM available.

## Toolchain (Windows host, Git Bash)
```bash
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_ZIGBUILD_ZIG_PATH="$(python -c "import ziglang,os;print(os.path.join(os.path.dirname(ziglang.__file__),'zig.exe'))")"
cargo zigbuild --release --target armv7-unknown-linux-musleabihf
```
- Rust host toolchain is `x86_64-pc-windows-gnu` (no MSVC installed).
- For C code use the `cc` crate in `build.rs`; with zigbuild the target C compiler is zig cc.
  If `cc` doesn't pick it up automatically, set `CC_armv7_unknown_linux_musleabihf` to a
  wrapper calling `zig cc -target arm-linux-musleabihf`.
- Release profile for spikes: `opt-level = 3`, `lto = true`, `strip = true` is fine.

## Device access
- Helper: `python E:/Temp/claude/E--Downloads-KinNovel/ae308a91-c8a1-42b1-93ab-656c7bd2a163/scratchpad/kssh.py run "<cmd>" [timeout_s]`,
  `... kssh.py put <local> <remote>`, `... kssh.py get <remote> <local>` (SSH root@192.168.1.6:2222, empty password).
- **Always `export MSYS_NO_PATHCONV=1`** before calling it, and give local paths in Windows form
  (`E:/Downloads/...`), remote paths in POSIX form (`/tmp/...`).
- Put binaries in `/tmp/` on the device (tmpfs; `/mnt/us` is vfat — no exec bit). `chmod +x` in /tmp works.
- Do NOT modify `/mnt/us/extensions/kinnovel` (the installed Python app) and don't leave processes running.

## Screen ownership (only the display spike needs this)
The Kindle framework (awesome/cvm) owns the framebuffer. The Python app's `bin/start.sh` shows how to
take it over: find PIDs holding `/dev/fb0`, `kill -STOP` them, save a snapshot, run, then restore
snapshot + `kill -CONT` + `lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home`.
Write your own small wrapper script based on it (in /tmp), always restoring on exit (trap).

## Network etiquette (LightNovelShelf API)
The test account can get banned. **At most 3 HTTP/WS requests per spike run, ≥ 6 s apart**, and
prefer unauthenticated endpoints. Never print or commit credentials.
Reference implementation of the protocol: `bin/src/kinnovel/transport.py` and `api.py`.

## Reporting
Each spike: commit nothing (leave files in `rust/spike-<name>/`, add `target/` to a local .gitignore),
and report: what works on device, measured numbers, binary size, crates/C libs used + licenses,
problems and recommended approach for the real implementation.
