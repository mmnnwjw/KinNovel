// FBInk is only compiled for the Linux (Kindle) target; host tests on other
// platforms use `MemoryDisplay` and don't need it. Source: git submodule at
// rust/third_party/FBInk, same defines/files as rust/spike-fbink/build.rs.
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=csrc/shim.c");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" {
        return;
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let fbink_dir = Path::new(&manifest_dir)
        .join("..")
        .join("..")
        .join("third_party")
        .join("FBInk");

    if !fbink_dir.join("fbink.c").exists() {
        panic!(
            "FBInk source not found at {:?}; `git submodule update --init` rust/third_party/FBInk first",
            fbink_dir
        );
    }

    cc::Build::new()
        .include(&fbink_dir)
        .file(fbink_dir.join("fbink.c"))
        .file(fbink_dir.join("cutef8").join("utf8.c"))
        .file(fbink_dir.join("cutef8").join("dfa.c"))
        .file(fbink_dir.join("fbink_input_scan.c"))
        .file("csrc/shim.c")
        .define("FBINK_FOR_KINDLE", None)
        .define("FBINK_MINIMAL", None)
        .define("FBINK_WITH_INPUT", None)
        .define("NDEBUG", None)
        .flag("-std=gnu11")
        .flag("-w")
        .opt_level(2)
        .compile("fbink_static");

    println!(
        "cargo:rerun-if-changed={}",
        fbink_dir.join("fbink.c").display()
    );
}
