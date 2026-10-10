//! 封面缩略图: 按 (url, w, h) 缓存已解码、已适配尺寸的灰度位图。
//!
//! 每个列表页自己持有一个 `CoverCache` (页面状态放页面结构体, 不用全局), 用法与
//! `pages::reader::ImageSlot` 一致: `request` 在后台线程解码 (磁盘缓存与 Python 版共用,
//! 见 `crate::net::image_bytes`), 结果通过 `cx.spawn` 投递回同一页面实例的 `on_message`。

use std::collections::HashMap;

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
    Failed,
}

#[derive(Default)]
pub struct CoverCache {
    entries: HashMap<(String, u32, u32), Slot>,
}

/// 后台解码结果; 页面在 `on_message` 里用 `CoverCache::on_loaded` 消费。
pub struct CoverLoaded {
    pub key: (String, u32, u32),
    pub result: Result<Option<Bitmap>, String>,
}

impl CoverCache {
    /// 没有已请求过就发起后台加载; 已有结果 (含失败/缺失) 不重复请求。
    pub fn request(&mut self, cx: &Cx<KinNovel>, url: &str, w: u32, h: u32) {
        if url.is_empty() {
            return;
        }
        let key = (url.to_string(), w, h);
        if self.entries.contains_key(&key) {
            return;
        }
        self.entries.insert(key.clone(), Slot::Pending);
        let paths: Paths = cx.app.paths.clone();
        let net: Option<Client> = cx.app.net();
        cx.spawn(move || {
            let (url, w, h) = (key.0.clone(), key.1, key.2);
            let result = crate::net::image_bytes(&paths, net.as_ref(), &url, h).and_then(|bytes| match bytes {
                None => Ok(None),
                Some(bytes) => kn_render::decode_gray(&bytes, Some((w, h))).map(|img| Some(img.fit_within(w, h))).map_err(|e| e.to_string()),
            });
            CoverLoaded { key, result }
        });
    }

    pub fn on_loaded(&mut self, loaded: CoverLoaded) {
        let slot = match loaded.result {
            Ok(Some(bmp)) => Slot::Ready(bmp),
            Ok(None) => Slot::Missing,
            Err(_) => Slot::Failed,
        };
        self.entries.insert(loaded.key, slot);
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
