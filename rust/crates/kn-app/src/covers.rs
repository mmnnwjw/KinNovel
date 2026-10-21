//! 封面缩略图: 按 (url, w, h) 缓存已解码、已适配尺寸的灰度位图。
//!
//! 每个列表页自己持有一个 `CoverCache` (页面状态放页面结构体, 不用全局), 用法与
//! `pages::reader::ImageSlot` 一致: `request` 在后台线程解码 (磁盘缓存与 Python 版共用,
//! 见 `crate::net::spawn_image`), 结果投递回同一页面实例的 `on_message` (页面出栈后还没开始的就不做了)。

use std::collections::HashMap;
use std::time::Instant;

use kn_net::Client;
use kn_render::Bitmap;
use kn_ui::Cx;

use crate::store::Paths;
use crate::KinNovel;

enum Slot {
    Pending,
    Ready(Bitmap),
    /// 本地没有缓存且离线 (或 fixture 模式), 不尝试下载
    Missing,
    /// 下载或解码失败的时刻: 过了 `crate::net::RETRY_AFTER` 再画到时重试 (网络可能已恢复)
    Failed(Instant),
}

/// 已解码封面最多占这么多内存 (KPW5 书架大封面约 120 KB 一张, 列表行的约 20 KB)
const CACHE_BYTES: usize = 8 * 1024 * 1024;
/// 没有位图的条目 (缺失 / 失败) 最多留这么多
const CACHE_ENTRIES: usize = 512;
/// 最近请求过的这么多条不淘汰 (比一屏能画的封面多得多, 正在显示的不会在绘制中途消失)
const KEEP_RECENT: usize = 64;

type Key = (String, u32, u32);

#[derive(Default)]
pub struct CoverCache {
    entries: HashMap<(String, u32, u32), Slot>,
    /// 每条最近一次被请求的序号 (绘制前都会 `request`, 所以就是最近一次显示)
    used: HashMap<Key, u64>,
    tick: u64,
}

/// 后台解码结果; 页面在 `on_message` 里用 `CoverCache::on_loaded` 消费。
pub struct CoverLoaded {
    pub key: (String, u32, u32),
    pub result: Result<Option<Bitmap>, String>,
}

impl CoverCache {
    /// 没有已请求过就发起后台加载; 已有结果 (含缺失) 不重复请求, 失败的过一段时间再试。
    pub fn request(&mut self, cx: &Cx<KinNovel>, url: &str, w: u32, h: u32) {
        if url.is_empty() {
            return;
        }
        let key = (url.to_string(), w, h);
        self.touch(&key);
        match self.entries.get(&key) {
            Some(Slot::Failed(at)) if at.elapsed() >= crate::net::RETRY_AFTER => {}
            Some(_) => return,
            None => {}
        }
        self.entries.insert(key.clone(), Slot::Pending);
        let paths: Paths = cx.app.paths.clone();
        let net: Option<Client> = cx.app.net();
        crate::net::spawn_image(cx, paths, net, key.0.clone(), crate::net::ImageSource::Normal(h), || true, move |bytes| {
            let result = bytes.and_then(|cached| match cached {
                None => Ok(None),
                Some(cached) => cached.decode(Some((w, h))).map(|img| Some(img.fit_within(w, h).dithered())).map_err(|e| e.to_string()),
            });
            CoverLoaded { key, result }
        });
    }

    pub fn on_loaded(&mut self, loaded: CoverLoaded) {
        let slot = match loaded.result {
            Ok(Some(bmp)) => Slot::Ready(bmp),
            Ok(None) => Slot::Missing,
            Err(e) => {
                eprintln!("[cover] {}: {e}", loaded.key.0);
                Slot::Failed(Instant::now())
            }
        };
        self.entries.insert(loaded.key, slot);
        self.evict();
    }

    fn touch(&mut self, key: &Key) {
        self.tick += 1;
        self.used.insert(key.clone(), self.tick);
    }

    /// 超出上限时按最近请求的先后淘汰最久没显示的 (正在加载的和最近请求的不动)。
    fn evict(&mut self) {
        let bytes = |slot: &Slot| match slot {
            Slot::Ready(bmp) => bmp.data().len(),
            _ => 0,
        };
        let mut total: usize = self.entries.values().map(bytes).sum();
        if total <= CACHE_BYTES && self.entries.len() <= CACHE_ENTRIES + KEEP_RECENT {
            return;
        }
        let protect = self.tick.saturating_sub(KEEP_RECENT as u64);
        let mut old: Vec<(u64, Key)> = self
            .entries
            .iter()
            .filter(|(_, slot)| !matches!(slot, Slot::Pending))
            .map(|(k, _)| (self.used.get(k).copied().unwrap_or(0), k.clone()))
            .filter(|(t, _)| *t <= protect)
            .collect();
        old.sort_unstable_by_key(|(t, _)| *t);
        let mut count = self.entries.len();
        for (_, key) in old {
            if total <= CACHE_BYTES && count <= CACHE_ENTRIES + KEEP_RECENT {
                break;
            }
            if let Some(slot) = self.entries.remove(&key) {
                total -= bytes(&slot);
                count -= 1;
            }
            self.used.remove(&key);
        }
    }

    pub fn get(&self, url: &str, w: u32, h: u32) -> Option<&Bitmap> {
        match self.entries.get(&(url.to_string(), w, h)) {
            Some(Slot::Ready(bmp)) => Some(bmp),
            _ => None,
        }
    }

}

/// 封面占位: 细描边圆角框, 里面什么都不画 (UI-DESIGN 对缺图的约定)。
pub fn placeholder(frame: &mut Bitmap, theme: &kn_ui::Theme, m: &kn_ui::widgets::Metrics, rect: kn_render::Rect) {
    frame.rounded_rect(rect, m.radius, None, Some(theme.mid), 2);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(i: usize) -> Key {
        (format!("u{i}"), 100, 150)
    }

    /// 像页面那样: 先请求 (这里只记序号), 再收到解码结果
    fn load(c: &mut CoverCache, i: usize, bmp: Option<Bitmap>) {
        c.touch(&key(i));
        c.on_loaded(CoverLoaded { key: key(i), result: Ok(bmp) });
    }

    #[test]
    fn decoded_covers_stay_within_the_byte_budget() {
        let mut c = CoverCache::default();
        // 每张 100 KB: 远超上限
        for i in 0..300 {
            load(&mut c, i, Some(Bitmap::new(100, 1024, 0)));
            // 一直在屏幕上的那张每次绘制都会请求
            c.touch(&key(0));
        }
        let total: usize = c.entries.values().map(|s| if let Slot::Ready(b) = s { b.data().len() } else { 0 }).sum();
        assert!(total <= CACHE_BYTES + KEEP_RECENT * 100 * 1024, "{total}");
        assert!(c.entries.len() < 300 && c.used.len() == c.entries.len());
        assert!(c.get("u0", 100, 150).is_some(), "正在显示的不淘汰");
        assert!(c.get("u299", 100, 150).is_some());
        assert!(c.get("u1", 100, 150).is_none(), "最久没显示的先淘汰");
        // 没有位图的条目只按数量限制
        for i in 1000..3000 {
            load(&mut c, i, None);
        }
        assert!(c.entries.len() <= CACHE_ENTRIES + KEEP_RECENT + 1);
        // 正在加载的不淘汰
        c.entries.insert(key(5000), Slot::Pending);
        for i in 3000..4000 {
            load(&mut c, i, None);
        }
        assert!(matches!(c.entries.get(&key(5000)), Some(Slot::Pending)));
    }
}
