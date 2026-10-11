#!/bin/sh
# 交叉编译到 Kindle (armv7 hard-float, 静态 musl)。主机测试: ./build.sh host-test [cargo test 参数]
set -e
export PATH="$HOME/.cargo/bin:$PATH"
# zig 来自 pip 的 ziglang 包 (Windows 上是 zig.exe; Linux CI 上是 zig, 也可以直接用 PATH 里的)
PYTHON=$(command -v python || command -v python3)
export CARGO_ZIGBUILD_ZIG_PATH="$("$PYTHON" -c "import ziglang,os;d=os.path.dirname(ziglang.__file__);print(os.path.join(d,'zig.exe' if os.name=='nt' else 'zig'))")"
cd "$(dirname "$0")"
case "${1:-kindle}" in
  kindle)
    cargo zigbuild --release --target armv7-unknown-linux-musleabihf -p kn-app
    # 去掉调试信息、保留符号表 (见 Cargo.toml [profile.release])。objcopy 来自 rustup (Windows gnu 工具链自带
    # rust-objcopy; 其它平台 `rustup component add llvm-tools`)
    BIN=target/armv7-unknown-linux-musleabihf/release/kinnovel
    OBJCOPY=$(ls "$(rustc --print sysroot)"/lib/rustlib/*/bin/rust-objcopy* "$(rustc --print sysroot)"/lib/rustlib/*/bin/llvm-objcopy* 2>/dev/null | head -n 1)
    if [ -n "$OBJCOPY" ]; then
      "$OBJCOPY" --strip-debug "$BIN" "$BIN.stripped" && mv -f "$BIN.stripped" "$BIN"
    else
      echo "warning: 找不到 llvm-objcopy, 设备程序保留了调试信息" >&2
    fi
    ;;
  # 主机测试也用 zig 链接: html5ever → parking_lot → windows-link 在 windows-gnu 上需要 dlltool + as,
  # 本机没有 mingw binutils, 普通 `cargo test` 会在链接 import 库时失败。
  host-test) shift; cargo-zigbuild test --target x86_64-pc-windows-gnu --workspace "$@" ;;
  # 主机预览二进制 (kinnovel.exe --preview)
  host) cargo-zigbuild zigbuild --release --target x86_64-pc-windows-gnu -p kn-app ;;
  *) echo "usage: $0 [kindle|host|host-test]"; exit 2 ;;
esac
