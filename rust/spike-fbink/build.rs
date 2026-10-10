use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let fbink_dir = Path::new(&manifest_dir)
        .join("..")
        .join("third_party")
        .join("FBInk");

    if !fbink_dir.join("fbink.c").exists() {
        panic!(
            "FBInk source not found at {:?}; clone https://github.com/NiLuJe/FBInk there first",
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
        .flag("-w") // FBInk itself is warning-clean upstream but not worth our time to police here
        .opt_level(2)
        .compile("fbink_static");

    println!("cargo:rerun-if-changed=csrc/shim.c");
    println!(
        "cargo:rerun-if-changed={}",
        fbink_dir.join("fbink.c").display()
    );
}
