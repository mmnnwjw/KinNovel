fn main() {
    let t = std::time::Instant::now();
    let uname = std::fs::read_to_string("/proc/version").unwrap_or_default();
    println!("hello from rust on kindle: {}", uname.trim());
    println!("startup-to-print: {:?}", t.elapsed());
}
