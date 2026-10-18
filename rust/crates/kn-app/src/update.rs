//! 检查新版本、下载并安装 (「我的 → 关于」和 KUAL 菜单「KinNovel 工具箱 → 检查更新」)。
//!
//! 启动后在后台检查一次 (设置「自动检查更新」, 默认开, 每天最多一次), 结果存在
//! `data/update.json` = `{"checked": unix 秒, "latest": "1.3.0"}`, 「我的」页读它显示提示。
//!
//! GitHub 在国内经常连不上, 所以版本信息和安装包都有多个来源, 依次尝试:
//! - 版本信息: Gitee 镜像仓库的发行版 API → GitHub Releases API。
//! - 安装包: Gitee 发行版附件 → GitHub 下载地址经加速镜像 → GitHub 原始地址。
//!   加速镜像是第三方服务, 只有拿到了 sha256 (Gitee 发行说明里的 `SHA256:` 行 / GitHub 资产的 digest) 才用它,
//!   下载后必须校验一致。Gitee 仓库由 CI 同步 (`.github/workflows/gitee-sync.yml`、release.yml)。
//! 只访问这些主机, 不碰轻书架服务器。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::store::{self, atomic_write, Paths};

const GITEE_LATEST: &str = "https://gitee.com/api/v5/repos/mmnnwjw/KinNovel/releases/latest";
const GITHUB_LATEST: &str = "https://api.github.com/repos/mmnnwjw/KinNovel/releases/latest";
const GITHUB_DOWNLOAD: &str = "https://github.com/mmnnwjw/KinNovel/releases/download";
/// GitHub 加速镜像 (前缀 + 完整的 GitHub 地址)。2026-10 实测可用; 失效了按顺序跳过。
const MIRRORS: &[&str] = &["https://gh-proxy.com/", "https://ghfast.top/", "https://gh.llkk.cc/"];
pub const PROJECT_PAGE: &str = "github.com/mmnnwjw/KinNovel";
const CHECK_INTERVAL_SECS: i64 = 24 * 3600;
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

fn load(paths: &Paths) -> (i64, Option<String>) {
    let v: Value = std::fs::read(paths.update_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    (v.get("checked").and_then(Value::as_i64).unwrap_or(0), v.get("latest").and_then(Value::as_str).map(str::to_string))
}

fn remember(paths: &Paths, version: &str) {
    let text = json!({"checked": store::unix_now(), "latest": version}).to_string();
    if let Err(e) = atomic_write(&paths.update_file(), text.as_bytes()) {
        eprintln!("[update] 无法保存检查结果: {e}");
    }
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(version.to_string());
}

/// 启动时调用: 读出上次的结果; 需要时 (`enabled` 且超过一天) 在后台重新检查。
pub fn start(paths: &Paths, net: Option<kn_net::Client>, enabled: bool) {
    let (checked, latest) = load(paths);
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = latest;
    let Some(net) = net.filter(|_| enabled) else { return };
    if (store::unix_now() - checked).abs() < CHECK_INTERVAL_SECS {
        return;
    }
    let paths = paths.clone();
    std::thread::spawn(move || match fetch_latest(&net) {
        Ok(release) => {
            eprintln!("[update] 最新版本 {} (当前 {}, 来自 {})", release.version, env!("CARGO_PKG_VERSION"), release.source);
            remember(&paths, &release.version);
        }
        Err(e) => eprintln!("[update] 检查失败: {e}"),
    });
}

/// 立即检查 (「关于」里的按钮), 在后台线程调用。返回最新的版本号。
pub fn check_now(paths: &Paths, net: Option<&kn_net::Client>) -> Result<String, String> {
    let release = check_release(paths, net)?;
    Ok(release.version)
}

/// 立即检查并返回完整的发布信息 (更新页用)。
pub fn check_release(paths: &Paths, net: Option<&kn_net::Client>) -> Result<Release, String> {
    let net = net.ok_or("离线模式")?;
    let release = fetch_latest(net)?;
    remember(paths, &release.version);
    Ok(release)
}

/// 一个发布版本。
#[derive(Clone, Debug, Default)]
pub struct Release {
    /// "1.6.2"
    pub version: String,
    /// 发行说明 (Markdown 原文)
    pub notes: String,
    /// 安装包的 sha256 (小写十六进制), 来源没给时为空
    pub sha256: Option<String>,
    /// 安装包下载地址, 按顺序尝试
    pub urls: Vec<String>,
    /// 版本信息来自哪里 ("Gitee" / "GitHub")
    pub source: &'static str,
}

fn package_name(version: &str) -> String {
    format!("KinNovel-v{version}.zip")
}

/// 文本里第一个 64 位十六进制串 (发行说明里的 "SHA256: …" 行, 或 GitHub 的 "sha256:…")。
fn find_sha256(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("sha256")?;
    lower[start + 6..]
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|w| w.len() == 64)
        .map(str::to_string)
}

/// Gitee 发行版 API 的响应 → Release (只取上传的附件, 不要自动生成的源码包)。
fn parse_gitee(v: &Value) -> Result<Release, String> {
    let version = v.get("tag_name").and_then(Value::as_str).ok_or("Gitee 响应里没有 tag_name")?.trim_start_matches('v').to_string();
    let notes = v.get("body").and_then(Value::as_str).unwrap_or("").to_string();
    let wanted = package_name(&version);
    let urls = v
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| a.get("browser_download_url").and_then(Value::as_str))
        .filter(|u| u.ends_with(&wanted))
        .map(str::to_string)
        .collect();
    let sha256 = notes.lines().find(|l| l.to_ascii_lowercase().contains("sha256")).and_then(find_sha256);
    Ok(Release { version, notes, sha256, urls, source: "Gitee" })
}

/// GitHub Releases API 的响应 → Release (资产的 digest 字段是 "sha256:…")。
fn parse_github(v: &Value) -> Result<Release, String> {
    let version = v.get("tag_name").and_then(Value::as_str).ok_or("GitHub 响应里没有 tag_name")?.trim_start_matches('v').to_string();
    let notes = v.get("body").and_then(Value::as_str).unwrap_or("").to_string();
    let wanted = package_name(&version);
    let asset = v
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|a| a.get("name").and_then(Value::as_str) == Some(wanted.as_str()));
    let sha256 = asset.and_then(|a| a.get("digest")).and_then(Value::as_str).and_then(find_sha256);
    Ok(Release { version, notes, sha256, urls: Vec::new(), source: "GitHub" })
}

/// 依次问 Gitee、GitHub; 再补上 GitHub 的下载地址 (有 sha256 时先走加速镜像)。
fn fetch_latest(net: &kn_net::Client) -> Result<Release, String> {
    let mut errors = Vec::new();
    let mut release = None;
    for (name, url, parse) in [("Gitee", GITEE_LATEST, parse_gitee as fn(&Value) -> Result<Release, String>), ("GitHub", GITHUB_LATEST, parse_github)] {
        let result = net
            .download(url, 512 * 1024)
            .map_err(|e| e.to_string())
            .and_then(|body| serde_json::from_slice::<Value>(&body).map_err(|e| e.to_string()))
            .and_then(|v| parse(&v));
        match result {
            Ok(r) => {
                release = Some(r);
                break;
            }
            Err(e) => {
                eprintln!("[update] {name}: {e}");
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    let mut release = release.ok_or_else(|| errors.join("; "))?;
    let github = format!("{GITHUB_DOWNLOAD}/v{0}/{1}", release.version, package_name(&release.version));
    if release.sha256.is_some() {
        release.urls.extend(MIRRORS.iter().map(|m| format!("{m}{github}")));
    }
    release.urls.push(github);
    Ok(release)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
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
/// (有的加速镜像会以几 KB/s 的涓流一直传, 读超时触发不了)。
const SOURCE_DEADLINE: Duration = Duration::from_secs(240);
const SLOW_CHECK_AFTER: Duration = Duration::from_secs(20);
const MIN_BYTES_PER_SEC: f64 = 12.0 * 1024.0;

/// 下载安装包: 按顺序换地址, 有 sha256 时校验。`progress` 大约每秒收到一次进度。
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
                    _ => {
                        eprintln!("[update] 从 {host} 下载了 {} 字节, {:.1} 秒", bytes.len(), started.elapsed().as_secs_f32());
                        return Ok(bytes);
                    }
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
    fn release_sources_parse() {
        let gitee = json!({
            "tag_name": "v1.7.0",
            "body": format!("## 新功能\n- x\n\nSHA256: {}\n", HASH.to_uppercase()),
            "assets": [
                {"browser_download_url": "https://gitee.com/mmnnwjw/KinNovel/releases/download/v1.7.0/KinNovel-v1.7.0.zip", "name": "KinNovel-v1.7.0.zip"},
                {"browser_download_url": "https://gitee.com/mmnnwjw/KinNovel/archive/refs/tags/v1.7.0.zip"}
            ]
        });
        let r = parse_gitee(&gitee).unwrap();
        assert_eq!(r.version, "1.7.0");
        assert_eq!(r.sha256.as_deref(), Some(HASH));
        assert_eq!(r.urls, vec!["https://gitee.com/mmnnwjw/KinNovel/releases/download/v1.7.0/KinNovel-v1.7.0.zip".to_string()]);
        let github = json!({"tag_name": "v1.7.0", "body": "", "assets": [{"name": "KinNovel-v1.7.0.zip", "digest": format!("sha256:{HASH}")}]});
        let r = parse_github(&github).unwrap();
        assert_eq!((r.version.as_str(), r.sha256.as_deref()), ("1.7.0", Some(HASH)));
        assert!(find_sha256("没有哈希").is_none());
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
