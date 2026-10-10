//! 打印输入串的前 12 个候选: `cargo run -p kn-ime --example candidates -- zhongguo g zg`
fn main() {
    let dict = kn_ime::dict();
    for raw in std::env::args().skip(1) {
        let start = std::time::Instant::now();
        let c = dict.candidates(&raw);
        let segs: Vec<String> = dict.segment(&raw).iter().map(|s| if s.full { s.text.clone() } else { format!("{}~", s.text) }).collect();
        let top: Vec<&str> = c.iter().take(12).map(|c| c.text.as_str()).collect();
        println!("{raw} [{}] ({} 个, {:.1} ms): {}", segs.join(" "), c.len(), start.elapsed().as_secs_f32() * 1e3, top.join(" "));
    }
}
