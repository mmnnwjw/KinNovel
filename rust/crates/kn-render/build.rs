// Linux (Kindle; CI 主机测试也走这里) 上把 libwebp 的解码器静态编进来: 它有 NEON 版的 IDCT / 环路滤波 /
// 缩放, 比纯 Rust 的 image-webp 快得多。源码是 git 子模块 rust/third_party/libwebp (v1.5.0),
// 只编解码相关的文件 (同 libwebp makefile.unix 的 DEC_OBJS / DSP_DEC_OBJS / UTILS_DEC_OBJS)。
// 其它平台 (Windows 主机预览) 不编, 用 image-webp。编进来时设置 cfg `kn_libwebp`。
use std::path::Path;

const DEC: &[&str] = &["alpha_dec", "buffer_dec", "frame_dec", "idec_dec", "io_dec", "quant_dec", "tree_dec", "vp8_dec", "vp8l_dec", "webp_dec"];
const DSP: &[&str] = &[
    "alpha_processing", "alpha_processing_neon", "alpha_processing_sse2", "alpha_processing_sse41", "cpu", "dec", "dec_clip_tables",
    "dec_neon", "dec_sse2", "dec_sse41", "filters", "filters_neon", "filters_sse2", "lossless", "lossless_neon", "lossless_sse2",
    "lossless_sse41", "rescaler", "rescaler_neon", "rescaler_sse2", "upsampling", "upsampling_neon", "upsampling_sse2",
    "upsampling_sse41", "yuv", "yuv_neon", "yuv_sse2", "yuv_sse41",
];
const UTILS: &[&str] = &[
    "bit_reader_utils", "color_cache_utils", "filters_utils", "huffman_utils", "palette", "quant_levels_dec_utils", "random_utils",
    "rescaler_utils", "thread_utils", "utils",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=csrc/webp_shim.c");
    println!("cargo::rustc-check-cfg=cfg(kn_libwebp)");
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "linux" {
        return;
    }
    let root = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../third_party/libwebp");
    if !root.join("src/dec/vp8_dec.c").exists() {
        panic!("libwebp source not found at {root:?}; run `git submodule update --init rust/third_party/libwebp`");
    }
    let mut build = cc::Build::new();
    build.include(&root).file("csrc/webp_shim.c").define("NDEBUG", None).flag("-w").opt_level(3);
    for (dir, names) in [("dec", DEC), ("dsp", DSP), ("utils", UTILS)] {
        for name in names {
            build.file(root.join("src").join(dir).join(format!("{name}.c")));
        }
    }
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "arm" {
        // 所有 ARMv7 Kindle 都有 NEON (同 .cargo/config.toml 的 +neon); 打开后 libwebp 编译 NEON 路径。
        // cargo-zigbuild 的 zig cc 包装自带 `-mcpu=generic+v7a+vfp3-d32+thumb2-neon` (明确关掉 NEON),
        // `-mfpu` 对它无效, 要用后面的 -mcpu 覆盖 (照抄成 `vfp3-d32+...+neon` 会让 clang 21 后端崩溃,
        // `vfp3` 本身就含 32 个双精度寄存器); 其它 (gcc/clang) 交叉编译器用 -mfpu=neon。
        let zig = build.get_compiler().path().to_string_lossy().contains("zig");
        build.flag(if zig { "-mcpu=generic+v7a+vfp3+neon" } else { "-mfpu=neon" });
    }
    build.compile("kn_webp");
    println!("cargo:rustc-cfg=kn_libwebp");
}
