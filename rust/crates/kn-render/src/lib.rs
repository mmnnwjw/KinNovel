//! kn-render: 8 位灰度位图、几何、文字与图片绘制。
//!
//! 约定:
//! - 像素为 L8, 0 = 黑, 255 = 白 (与 Kindle framebuffer 一致)。
//! - 所有绘制函数都裁剪到位图边界, 越界坐标不会 panic。
//! - 本 crate 不依赖平台, 全部可在主机上单测。

mod bitmap;
mod geom;
pub mod image;
mod text;

pub use bitmap::Bitmap;
pub use geom::{Point, Rect};
pub use image::{decode_gray, height_bucket, ImageError};
pub use text::{decode_woff2, FontId, FontStore, FxHasher, FxMap, GlyphCache, LineMetrics, TextError, TextStyle};
