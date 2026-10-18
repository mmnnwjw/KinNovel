//! 检查新版本、下载并安装 (启动时自动检查 + 弹窗、「我的 → 关于」、KUAL 菜单「KinNovel 工具箱 → 检查更新」)。
//!
//! 启动后在后台检查一次 (设置「自动检查更新」, 默认开, 每天最多一次), 结果存在
//! `data/update.json` = `{"checked": unix 秒, "latest": "1.3.0", "prompted": "1.3.0"}`:
//! 发现新版本时在首页类页面上弹一次提示 (每个版本只弹一次, `prompted`), 「我的 → 关于」一直有提示。
//!
//! GitHub 在国内经常连不上, 所以还有自己的更新服务器 (Cloudflare R2 桶 + 自定义域名, 发布时 CI 上传,
//! `tools/r2_publish.py`):
//! - 版本信息: 更新服务器的 `releases.json` (与 GitHub 发行版列表同样的格式) 和 GitHub 的发行版列表**同时**查,
//!   一边先回来后另一边最多再等几秒, 两边合并取最高的版本; 比当前新的所有版本的说明都给更新页显示。
//! - 安装包: 更新服务器 → GitHub。有 sha256 (资产的 digest) 时下载后必须校验一致。
//! - 签名: 每个安装包旁边有 `KinNovel-vX.Y.Z.zip.sig` (CI 用 Ed25519 私钥签的「包名 + sha256」,
//!   `tools/sign_update.py`), 两个来源都放。验签不过的包一律不装, 所以更新服务器的桶或域名
//!   被别人控制了也推不了假更新 (版本列表本身不签, 最多显示一个装不上的假版本)。
//! 只访问这些主机, 不碰轻书架服务器。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::store::{self, atomic_write, Paths};

const GITHUB_LIST: &str = "https://api.github.com/repos/mmnnwjw/KinNovel/releases?per_page=20";
const GITHUB_DOWNLOAD: &str = "https://github.com/mmnnwjw/KinNovel/releases/download";
/// 自己的更新服务器: `releases.json` + `KinNovel-vX.Y.Z.zip`
const UPDATE_SERVER: &str = "https://update.mmnnwjw.dpdns.org";
/// 更新页「来自 …」里的名字 (域名太长, 放不下)
const UPDATE_SERVER_NAME: &str = "更新服务器";
/// 一个来源先回来后, 另一个最多再等这么久 (国内连 GitHub 常常要等到 30 秒超时)
const SECOND_SOURCE_GRACE: Duration = Duration::from_secs(5);
pub const PROJECT_PAGE: &str = "github.com/mmnnwjw/KinNovel";
const CHECK_INTERVAL_SECS: i64 = 24 * 3600;
/// 发布签名公钥 (Ed25519)。私钥只在 CI (仓库 Secret `UPDATE_SIGNING_KEY`) 和作者本机。
const SIGNING_PUBLIC_KEY: &str = "579c991f5684ee31379fdaaed587de2a0ee1147c7e8ee2d757bb03fc2008d9df";
/// 发布包上限 (现在约 3.5 MB)
const PACKAGE_LIMIT: usize = 32 * 1024 * 1024;

/// 本次运行里已知的最新版本 (启动时从文件读, 后台检查完更新)。
static LATEST: Mutex<Option<String>> = Mutex::new(None);

impl Paths {
    fn update_file(&self) -> PathBuf {
        self.data_dir().join("update.json")
    }
}

/// "v1.2.10" / "1.2.10" → (1, 2, 10); 预发布后缀 ("-rc1") 忽略。
fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let text = text.trim().trim_start_matches('v');
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)))
}

/// `latest` 比当前程序新时返回它 (去掉 "v")。
pub fn newer_than_current(latest: &str) -> Option<String> {
    let current = parse_version(env!("CARGO_PKG_VERSION"))?;
    (parse_version(latest)? > current).then(|| latest.trim().trim_start_matches('v').to_string())
}

/// 已知的更新版本 (没有或不比当前新则 None)。
pub fn available() -> Option<String> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).as_deref().and_then(newer_than_current)
}

fn load_state(paths: &Paths) -> serde_json::Map<String, Value> {
    std::fs::read(paths.update_file())
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn save_state(paths: &Paths, state: &serde_json::Map<String, Value>) {
    if let Err(e) = atomic_write(&paths.update_file(), Value::Object(state.clone()).to_string().as_bytes()) {
        eprintln!("[update] 无法保存检查结果: {e}");
    }
}

fn remember(paths: &Paths, version: &str) {
    let mut state = load_state(paths);
    state.insert("checked".into(), store::unix_now().into());
    state.insert("latest".into(), version.into());
    save_state(paths, &state);
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(version.to_string());
}

/// 启动时调用: 读出上次的结果。返回是否该在后台重新检查 (`enabled` 且距上次超过一天)。
pub fn start(paths: &Paths, enabled: bool) -> bool {
    let state = load_state(paths);
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = state.get("latest").and_then(Value::as_str).map(str::to_string);
    let checked = state.get("checked").and_then(Value::as_i64).unwrap_or(0);
    enabled && (store::unix_now() - checked).abs() >= CHECK_INTERVAL_SECS
}

/// 有新版本且还没为它弹过提示时返回这个版本。
pub fn unprompted(paths: &Paths) -> Option<String> {
    let version = available()?;
    (load_state(paths).get("prompted").and_then(Value::as_str) != Some(version.as_str())).then_some(version)
}

/// 记下已经为 `version` 弹过提示 (每个版本只弹一次)。
pub fn mark_prompted(paths: &Paths, version: &str) {
    let mut state = load_state(paths);
    state.insert("prompted".into(), version.into());
    save_state(paths, &state);
}

/// 启动时的后台检查 (在后台线程调用): 更新已知的最新版本。
pub fn background_check(paths: &Paths, net: &kn_net::Client) {
    match fetch_latest(net) {
        Ok(release) => {
            eprintln!("[update] 最新版本 {} (当前 {}, 来自 {})", release.version, env!("CARGO_PKG_VERSION"), release.source);
            remember(paths, &release.version);
        }
        Err(e) => eprintln!("[update] 检查失败: {e}"),
    }
}

/// 立即检查 (「关于」里的按钮), 在后台线程调用。返回最新的版本号。
pub fn check_now(paths: &Paths, net: Option<&kn_net::Client>) -> Result<String, String> {
    let release = check_release(paths, net)?;
    Ok(release.version)
}

/// 立即检查并返回完整的发布信息 (更新页用; 每次进更新页都重新查, 不用缓存的版本号)。
pub fn check_release(paths: &Paths, net: Option<&kn_net::Client>) -> Result<Release, String> {
    let net = net.ok_or("离线模式")?;
    let release = fetch_latest(net)?;
    remember(paths, &release.version);
    Ok(release)
}

/// 一个版本的更新说明。
#[derive(Clone, Debug, Default)]
pub struct VersionNotes {
    pub version: String,
    /// 发布日期 "2026-10-17" (来源没给时为空)
    pub date: String,
    /// Markdown 原文
    pub notes: String,
}

/// 要安装的版本 (发行版列表里最高的)。
#[derive(Clone, Debug, Default)]
pub struct Release {
    /// "1.6.2"
    pub version: String,
    /// 比当前程序新的所有版本的说明, 新的在前 (没有更新时只有最新这一个)
    pub changes: Vec<VersionNotes>,
    /// 安装包的 sha256 (小写十六进制), 来源没给时为空
    pub sha256: Option<String>,
    /// 安装包下载地址, 按顺序尝试
    pub urls: Vec<String>,
    /// 版本信息来自哪里 (更新页显示)
    pub source: String,
}

/// 发行版列表里的一个版本
#[derive(Clone, Debug, Default)]
struct Listed {
    version: String,
    date: String,
    notes: String,
    sha256: Option<String>,
}

fn package_name(version: &str) -> String {
    format!("KinNovel-v{version}.zip")
}

/// 文本里第一个 64 位十六进制串 (GitHub 资产的 digest "sha256:…")。
fn find_sha256(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("sha256")?;
    lower[start + 6..]
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|w| w.len() == 64)
        .map(str::to_string)
}

fn listed_common(v: &Value) -> Option<Listed> {
    if v.get("prerelease").and_then(Value::as_bool) == Some(true) || v.get("draft").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let version = v.get("tag_name").and_then(Value::as_str)?.trim_start_matches('v').to_string();
    parse_version(&version)?;
    let date = ["published_at", "created_at"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(|d| d.chars().take(10).collect())
        .unwrap_or_default();
    let notes = v.get("body").and_then(Value::as_str).unwrap_or("").to_string();
    Some(Listed { version, date, notes, sha256: None })
}

/// GitHub 发行版列表 → 各版本 (资产的 digest 字段是 "sha256:…")。
fn parse_github(v: &Value) -> Vec<Listed> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let mut l = listed_common(r)?;
            let wanted = package_name(&l.version);
            l.sha256 = r
                .get("assets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|a| a.get("name").and_then(Value::as_str) == Some(wanted.as_str()))
                .and_then(|a| a.get("digest"))
                .and_then(Value::as_str)
                .and_then(find_sha256);
            Some(l)
        })
        .collect()
}

/// 发行版列表 → 要安装的版本: 取最高版本, 收集比当前新的所有版本的说明; 安装包地址 更新服务器 → GitHub。
/// 列表可以是两个来源拼起来的: 同一版本取有 sha256 的那条。
fn latest_of(list: &[Listed]) -> Option<Release> {
    let mut by_version: BTreeMap<(u32, u32, u32), &Listed> = BTreeMap::new();
    for l in list {
        let Some(key) = parse_version(&l.version) else { continue };
        let slot = by_version.entry(key).or_insert(l);
        if slot.sha256.is_none() && l.sha256.is_some() {
            *slot = l;
        }
    }
    let (_, latest) = by_version.iter().next_back()?;
    let current = parse_version(env!("CARGO_PKG_VERSION")).unwrap_or_default();
    let mut changes: Vec<VersionNotes> = by_version
        .iter()
        .rev()
        .filter(|(k, _)| **k > current)
        .map(|(_, l)| VersionNotes { version: l.version.clone(), date: l.date.clone(), notes: l.notes.clone() })
        .collect();
    if changes.is_empty() {
        changes.push(VersionNotes { version: latest.version.clone(), date: latest.date.clone(), notes: latest.notes.clone() });
    }
    let name = package_name(&latest.version);
    // 更新服务器的地址带上 sha256: Cloudflare 默认缓存 .zip, 同一版本原地替换过时旧包还留在边缘节点上,
    // 地址随内容变就不会取到旧的
    let server = match &latest.sha256 {
        Some(sha) => format!("{UPDATE_SERVER}/{name}?sha256={sha}"),
        None => format!("{UPDATE_SERVER}/{name}"),
    };
    let urls = vec![server, format!("{GITHUB_DOWNLOAD}/v{0}/{name}", latest.version)];
    Some(Release { version: latest.version.clone(), changes, sha256: latest.sha256.clone(), urls, source: String::new() })
}

fn fetch_list(net: &kn_net::Client, url: &str, parse: fn(&Value) -> Vec<Listed>) -> Result<Vec<Listed>, String> {
    let body = net.download(url, 2 * 1024 * 1024).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let list = parse(&v);
    if list.is_empty() {
        return Err("没有发布版本".into());
    }
    Ok(list)
}

/// 同时问更新服务器和 GitHub, 合并结果。
fn fetch_latest(net: &kn_net::Client) -> Result<Release, String> {
    // releases.json 上传时设了 no-cache; 再加个时间戳, 以后给域名开了缓存也拿到的是最新的
    let server = format!("{UPDATE_SERVER}/releases.json?t={}", store::unix_now());
    let (tx, rx) = mpsc::channel();
    for (name, url) in [(UPDATE_SERVER_NAME, server), ("GitHub", GITHUB_LIST.to_string())] {
        let (tx, net) = (tx.clone(), net.clone());
        std::thread::spawn(move || {
            let _ = tx.send((name, fetch_list(&net, &url, parse_github)));
        });
    }
    drop(tx);
    let mut list = Vec::new();
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    for _ in 0..2 {
        let received = if sources.is_empty() { rx.recv().ok() } else { rx.recv_timeout(SECOND_SOURCE_GRACE).ok() };
        let Some((name, result)) = received else { break };
        match result {
            Ok(l) => {
                list.extend(l);
                sources.push(name);
            }
            Err(e) => {
                eprintln!("[update] {name}: {e}");
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    sources.sort_by_key(|n| *n != UPDATE_SERVER_NAME);
    latest_of(&list)
        .map(|mut r| {
            r.source = sources.join(" + ");
            r
        })
        .ok_or_else(|| if errors.is_empty() { "没有发布版本".to_string() } else { errors.join("; ") })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_bytes<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// 签名的内容: 包名 (含版本号, 旧版本的包不能冒充新版本) + sha256。与 `tools/sign_update.py` 一致。
fn signed_message(package: &str, sha256: &str) -> String {
    format!("kinnovel-update-v1\n{package}\n{sha256}\n")
}

/// `signature` = `.sig` 文件内容 (128 位十六进制)
fn signature_valid(public_key: &str, package: &str, sha256: &str, signature: &str) -> bool {
    use ed25519_dalek::{Signature, VerifyingKey};
    let (Some(key), Some(sig)) = (hex_bytes::<32>(public_key), hex_bytes::<64>(signature)) else { return false };
    let Ok(key) = VerifyingKey::from_bytes(&key) else { return false };
    key.verify_strict(signed_message(package, sha256).as_bytes(), &Signature::from_bytes(&sig)).is_ok()
}

/// 安装包地址 → 签名地址 (查询参数留在后面)
fn sig_url(url: &str) -> String {
    match url.split_once('?') {
        Some((path, query)) => format!("{path}.sig?{query}"),
        None => format!("{url}.sig"),
    }
}

/// 从各来源取 `.sig` 验签, 有一个通过就行。
fn check_signature(net: &kn_net::Client, release: &Release, sha256: &str) -> Result<(), String> {
    let package = package_name(&release.version);
    let mut errors = Vec::new();
    for url in &release.urls {
        let host = url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url);
        match net.download(&sig_url(url), 4096) {
            Ok(sig) if signature_valid(SIGNING_PUBLIC_KEY, &package, sha256, &String::from_utf8_lossy(&sig)) => return Ok(()),
            Ok(_) => errors.push(format!("{host}: 签名不符")),
            Err(e) => errors.push(format!("{host}: 签名 {e}")),
        }
    }
    Err(errors.join("; "))
}

/// 下载进度 (更新页显示)
#[derive(Clone, Debug, Default)]
pub struct DownloadProgress {
    /// 正在用的来源 (主机名)
    pub host: String,
    pub received: usize,
    pub total: Option<usize>,
}

/// 一个来源最多用这么久; 开始一段时间后平均速度还低于下限就换下一个
/// (有的线路会以几 KB/s 的涓流一直传, 读超时触发不了)。
const SOURCE_DEADLINE: Duration = Duration::from_secs(240);
const SLOW_CHECK_AFTER: Duration = Duration::from_secs(20);
const MIN_BYTES_PER_SEC: f64 = 12.0 * 1024.0;

/// 下载安装包: 按顺序换地址, 有 sha256 时校验, 再验签名。`progress` 大约每秒收到一次进度。
pub fn download(net: &kn_net::Client, release: &Release, mut progress: impl FnMut(&DownloadProgress)) -> Result<Vec<u8>, String> {
    let mut errors = Vec::new();
    for url in &release.urls {
        let host = url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url).to_string();
        let mut state = DownloadProgress { host: host.clone(), received: 0, total: None };
        progress(&state);
        let started = Instant::now();
        let mut reported = Instant::now();
        let mut too_slow = false;
        let result = net.download_with(url, PACKAGE_LIMIT, &mut |received, total| {
            let elapsed = started.elapsed();
            if elapsed > SOURCE_DEADLINE || (elapsed > SLOW_CHECK_AFTER && (received as f64) < MIN_BYTES_PER_SEC * elapsed.as_secs_f64()) {
                too_slow = true;
                return false;
            }
            if reported.elapsed() >= Duration::from_secs(1) {
                reported = Instant::now();
                state.received = received;
                state.total = total;
                progress(&state);
            }
            true
        });
        match result {
            Ok(bytes) => {
                let actual = sha256_hex(&bytes);
                match &release.sha256 {
                    Some(expected) if *expected != actual => {
                        eprintln!("[update] {host}: sha256 不符 ({actual})");
                        errors.push(format!("{host}: 校验不符"));
                    }
                    _ => match check_signature(net, release, &actual) {
                        Ok(()) => {
                            eprintln!("[update] 从 {host} 下载了 {} 字节, {:.1} 秒, 签名有效", bytes.len(), started.elapsed().as_secs_f32());
                            return Ok(bytes);
                        }
                        Err(e) => {
                            eprintln!("[update] {host}: 安装包签名无效 ({e})");
                            errors.push(format!("{host}: 签名无效"));
                        }
                    },
                }
            }
            Err(e) => {
                let e = if too_slow { "太慢, 换下一个来源".to_string() } else { e.to_string() };
                eprintln!("[update] {host}: {e}");
                errors.push(format!("{host}: {e}"));
            }
        }
    }
    Err(errors.join("; "))
}

/// 程序所在的扩展目录 (启动脚本传 `KN_PROGRAM_DIR`; 平时与数据目录相同)。
pub fn program_dir(paths: &Paths) -> PathBuf {
    std::env::var_os("KN_PROGRAM_DIR").map(PathBuf::from).unwrap_or_else(|| paths.app_dir.clone())
}

/// 程序退出后由启动脚本处理的请求 (文件在 /tmp)
pub const RESTART_MARKER: &str = "/tmp/kinnovel_restart";
pub const UNINSTALL_MARKER: &str = "/tmp/kinnovel_uninstall";

/// 把发布包装进 `dir`: 校验内容与版本, 先全部写成 `*.kn-new`, 再逐个改名替换 (vfat 上 rename 会替换旧文件;
/// 正在运行的启动脚本和 /tmp 里的程序副本不受影响, 新版下次启动生效)。数据 (data/ cache/ logs/) 不动。
pub fn install(zip: &[u8], version: &str, dir: &Path) -> Result<usize, String> {
    let entries = unzip(zip)?;
    let files: Vec<(String, Vec<u8>)> = entries
        .into_iter()
        .filter_map(|(name, data)| {
            let rel = name.strip_prefix("KinNovel/")?;
            (!rel.is_empty() && !rel.ends_with('/')).then(|| (rel.to_string(), data))
        })
        .collect();
    for (rel, _) in &files {
        if rel.starts_with('/') || rel.contains('\\') || rel.split('/').any(|p| p == ".." || p.is_empty()) {
            return Err(format!("安装包里有可疑路径: {rel}"));
        }
    }
    let get = |name: &str| files.iter().find(|(r, _)| r == name).map(|(_, d)| d);
    for required in ["bin/kinnovel", "bin/kinnovel.sh", "config.xml", "menu.json"] {
        if get(required).is_none() {
            return Err(format!("安装包缺少 {required}"));
        }
    }
    let config = String::from_utf8_lossy(get("config.xml").expect("checked"));
    if !config.contains(&format!("<version>{version}</version>")) {
        return Err(format!("安装包版本与 {version} 不符"));
    }
    let staged: Vec<(PathBuf, PathBuf)> = files
        .iter()
        .map(|(rel, data)| {
            let target = dir.join(rel);
            let tmp = PathBuf::from(format!("{}.kn-new", target.display()));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
            Ok((tmp, target))
        })
        .collect::<Result<_, String>>()
        .inspect_err(|_| {
            for (rel, _) in &files {
                let _ = std::fs::remove_file(format!("{}.kn-new", dir.join(rel).display()));
            }
        })?;
    for (tmp, target) in &staged {
        std::fs::rename(tmp, target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(staged.len())
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// 最小的 zip 读取 (发布包由 tools/package_release.py 用 deflate 打包, 不用 zip64): (名字, 内容), 每个都校验 CRC。
fn unzip(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    const EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    let bad = || "安装包不是有效的 zip".to_string();
    let eocd = (0..zip.len().saturating_sub(21)).rev().take(65_557).find(|&i| zip[i..].starts_with(&EOCD)).ok_or_else(bad)?;
    let count = u16_at(zip, eocd + 10).ok_or_else(bad)?;
    let mut at = u32_at(zip, eocd + 16).ok_or_else(bad)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(zip, at) != Some(0x0201_4b50) {
            return Err(bad());
        }
        let method = u16_at(zip, at + 10).ok_or_else(bad)?;
        let crc = u32_at(zip, at + 16).ok_or_else(bad)? as u32;
        let packed = u32_at(zip, at + 20).ok_or_else(bad)?;
        let size = u32_at(zip, at + 24).ok_or_else(bad)?;
        let (name_len, extra_len, comment_len) = (u16_at(zip, at + 28).ok_or_else(bad)?, u16_at(zip, at + 30).ok_or_else(bad)?, u16_at(zip, at + 32).ok_or_else(bad)?);
        let local = u32_at(zip, at + 42).ok_or_else(bad)?;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?).to_string();
        at += 46 + name_len + extra_len + comment_len;
        if u32_at(zip, local) != Some(0x0403_4b50) {
            return Err(bad());
        }
        let start = local + 30 + u16_at(zip, local + 26).ok_or_else(bad)? + u16_at(zip, local + 28).ok_or_else(bad)?;
        let raw = zip.get(start..start + packed).ok_or_else(bad)?;
        let data = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, size.max(1)).map_err(|_| format!("{name}: 解压失败"))?,
            m => return Err(format!("{name}: 不支持的压缩方式 {m}")),
        };
        if data.len() != size || crc32fast::hash(&data) != crc {
            return Err(format!("{name}: 校验失败"));
        }
        out.push((name, data));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v1.2.10"), Some((1, 2, 10)));
        assert_eq!(parse_version("2.0"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.3.0-rc1"), Some((1, 3, 0)));
        assert_eq!(parse_version("x"), None);
        assert!(newer_than_current("v999.0.0").is_some());
        assert!(newer_than_current(env!("CARGO_PKG_VERSION")).is_none());
        assert!(newer_than_current("0.1.0").is_none());
    }

    const HASH: &str = "9128847c9a4c2ed440dfadc0e6b1d4f7ad75ae02d90d3118d09a590834346a36";

    #[test]
    fn release_list_gives_the_highest_version_and_newer_notes() {
        let current = env!("CARGO_PKG_VERSION");
        let (ma, mi, pa) = parse_version(current).unwrap();
        let next = format!("{ma}.{mi}.{}", pa + 1);
        let next2 = format!("{ma}.{mi}.{}", pa + 2);
        let github = json!([
            {"tag_name": format!("v{next}"), "published_at": "2026-10-20T10:00:00Z", "body": "## 新功能\n- a", "assets": []},
            {"tag_name": format!("v{next2}"), "published_at": "2026-10-21T01:00:00Z", "body": "- b",
             "assets": [{"name": format!("KinNovel-v{next2}.zip"), "digest": format!("sha256:{HASH}")}]},
            {"tag_name": format!("v{current}"), "body": "old", "assets": []},
            {"tag_name": "v99.0.0-rc1", "prerelease": true, "body": "", "assets": []}
        ]);
        let r = latest_of(&parse_github(&github)).unwrap();
        assert_eq!(r.version, next2);
        assert_eq!(r.sha256.as_deref(), Some(HASH));
        let versions: Vec<_> = r.changes.iter().map(|c| (c.version.as_str(), c.date.as_str())).collect();
        assert_eq!(versions, vec![(next2.as_str(), "2026-10-21"), (next.as_str(), "2026-10-20")]);
        assert_eq!(r.urls[0], format!("https://update.mmnnwjw.dpdns.org/KinNovel-v{next2}.zip?sha256={HASH}"));
        assert_eq!(sig_url(&r.urls[0]), format!("https://update.mmnnwjw.dpdns.org/KinNovel-v{next2}.zip.sig?sha256={HASH}"));
        assert_eq!(sig_url(&r.urls[1]), format!("{}.sig", r.urls[1]));
        assert!(r.urls[1].starts_with("https://github.com/"));
        // 两个来源合并: 同一版本取有 sha256 的那条, 版本取两边最高
        let server = json!([{"tag_name": format!("v{next}"), "body": "", "assets": []}]);
        let mut both = parse_github(&server);
        both.extend(parse_github(&github));
        let r = latest_of(&both).unwrap();
        assert_eq!(r.version, next2);
        assert_eq!(r.sha256.as_deref(), Some(HASH));
        assert_eq!(r.changes.len(), 2);
        assert!(latest_of(&[]).is_none());
        assert!(find_sha256("没有哈希").is_none());
    }

    #[test]
    fn signatures_bind_package_and_hash() {
        use ed25519_dalek::{Signer, SigningKey};
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let public: String = key.verifying_key().to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let sign = |package: &str, sha: &str| -> String {
            key.sign(signed_message(package, sha).as_bytes()).to_bytes().iter().map(|b| format!("{b:02x}")).collect()
        };
        let sig = sign("KinNovel-v9.0.0.zip", HASH);
        assert!(signature_valid(&public, "KinNovel-v9.0.0.zip", HASH, &format!("{sig}\n")));
        // 换了包名 (旧包冒充新版本) / 换了内容 / 别的密钥 / 乱码
        assert!(!signature_valid(&public, "KinNovel-v9.0.1.zip", HASH, &sig));
        assert!(!signature_valid(&public, "KinNovel-v9.0.0.zip", &HASH.replace('9', "8"), &sig));
        assert!(!signature_valid(SIGNING_PUBLIC_KEY, "KinNovel-v9.0.0.zip", HASH, &sig));
        assert!(!signature_valid(&public, "KinNovel-v9.0.0.zip", HASH, "zz"));
    }

    /// 与 tools/sign_update.py 的输出一致 (同一个固定种子)
    #[test]
    fn signature_format_matches_the_python_signer() {
        // python: SigningKey(bytes([7]*32)).sign(b"kinnovel-update-v1\nKinNovel-v9.0.0.zip\n" + HASH + b"\n").signature.hex()
        let public = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
        let sig = include_str!("../tests/fixtures/update_sig_v9.txt");
        assert!(signature_valid(public, "KinNovel-v9.0.0.zip", HASH, sig));
    }

    /// 用 Python zipfile 同样的格式 (deflate) 打一个包: 这里手写一个只含 stored 条目的 zip 加一个 deflate 条目。
    fn make_zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in files {
            let packed = if *deflate { miniz_oxide::deflate::compress_to_vec(data, 9) } else { data.to_vec() };
            let crc = crc32fast::hash(data);
            let offset = out.len() as u32;
            let method: u16 = if *deflate { 8 } else { 0 };
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&packed);
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0, 0, 0, 0]);
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn install_replaces_program_files_only() {
        let dir = std::env::temp_dir().join(format!("kn-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("bin/kinnovel"), b"old").unwrap();
        std::fs::write(dir.join("data/settings.json"), b"{}").unwrap();
        let big = vec![b'x'; 10_000];
        let zip = make_zip(&[
            ("KinNovel/", b"", false),
            ("KinNovel/bin/kinnovel", &big, true),
            ("KinNovel/bin/kinnovel.sh", b"#!/bin/sh\n", false),
            ("KinNovel/config.xml", b"<version>1.7.0</version>", true),
            ("KinNovel/menu.json", b"{}", false),
        ]);
        assert!(install(&zip, "1.8.0", &dir).unwrap_err().contains("不符"));
        assert_eq!(install(&zip, "1.7.0", &dir), Ok(4));
        assert_eq!(std::fs::read(dir.join("bin/kinnovel")).unwrap(), big);
        assert_eq!(std::fs::read(dir.join("data/settings.json")).unwrap(), b"{}");
        assert!(!dir.join("bin/kinnovel.kn-new").exists());
        let evil = make_zip(&[("KinNovel/../x", b"", false)]);
        assert!(install(&evil, "1.7.0", &dir).is_err());
        // 内容损坏 (stored 条目的一个字节) → CRC 不符
        let mut broken = zip.clone();
        let at = broken.windows(9).position(|w| w == b"#!/bin/sh").unwrap();
        broken[at + 2] ^= 0xff;
        assert!(unzip(&broken).unwrap_err().contains("校验失败"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
