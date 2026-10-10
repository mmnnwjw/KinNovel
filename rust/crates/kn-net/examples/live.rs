//! 对真实服务器做**一次**匿名调用 (GetLatestBookList), 验证 negotiate / TLS / SignalR / gzip 全链路。
//! 不登录、不读任何凭据 (session 指向一个不存在的临时文件)。测试账号有封禁风险: 不要循环调用。
//!
//!   cargo run -p kn-net --example live

use std::time::{Duration, Instant};

fn main() {
    let session = std::env::temp_dir().join(format!("kn-live-session-{}.json", std::process::id()));
    let client = kn_net::Client::new(kn_net::ClientConfig { session_path: session, ..Default::default() });
    let started = Instant::now();
    let params = serde_json::json!({"Page": 1, "Size": 5, "IgnoreJapanese": false, "IgnoreAI": false});
    match client.invoke("GetLatestBookList", params, 0, Duration::ZERO) {
        Ok(v) => {
            let items = v.get("Data").and_then(|d| d.as_array()).map(|a| a.len()).unwrap_or(0);
            let first = v.pointer("/Data/0/Title").and_then(|t| t.as_str()).unwrap_or("<none>");
            println!("GetLatestBookList ok in {:?}: {items} item(s), first: {first}", started.elapsed());
        }
        Err(e) => println!("GetLatestBookList failed after {:?}: {e:?}", started.elapsed()),
    }
    client.shutdown();
}
