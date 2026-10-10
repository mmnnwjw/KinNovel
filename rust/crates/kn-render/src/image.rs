//! 图片解码: JPEG / PNG → L8 灰度 `Bitmap` (墨水屏只需要灰度)。
//!
//! JPEG 用 DCT 缩放解码 (1/2、1/4、1/8), 只解到 "不小于目标尺寸" 的最小档 —— 1200×2234 的插图
//! 放进 ~800 px 高的区域时只解 1/2, 设备上省下大半时间 (对应 Python 版 Pillow 的 `draft()`)。
//! 之后再用 `Bitmap::fit_within` 区域平均缩到最终尺寸。
//!
//! 只取亮度 (墨水屏是灰度, 色度算出来也是白算):
//! - YCbCr 存储的 JPEG: 关掉解码器的颜色转换, 直接取 Y 通道 (JFIF 的 Y 是全范围, 即 BT.601 灰度);
//!   RGB 存储的 (Adobe transform 0 或分量 id 为 R/G/B) 仍走 RGB → 灰度。
//! - 有损 WebP (无透明、非动图): 直接调 VP8 解码取亮度平面, 跳过 YUV → RGB 上采样;
//!   VP8 的亮度是 16–235 的窄范围, 按 1.164 × (Y − 16) 拉伸 (与 RGB 转换的系数一致)。
//!   无损 / 带透明 / 动图仍走完整解码。

use std::io::Cursor;

use crate::bitmap::Bitmap;

#[derive(Debug)]
pub enum ImageError {
    Unsupported,
    Decode(String),
    /// 解码后像素太多 (防止恶意/异常图片把内存撑爆)
    TooLarge,
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Unsupported => write!(f, "不支持的图片格式"),
            ImageError::Decode(e) => write!(f, "图片解码失败: {e}"),
            ImageError::TooLarge => write!(f, "图片过大"),
        }
    }
}

impl std::error::Error for ImageError {}

/// 解码后最多 32 M 像素 (灰度 32 MB)。
const MAX_PIXELS: u64 = 32 * 1024 * 1024;

/// RGB → 灰度 (ITU-R BT.601, 整数近似)。
#[inline]
fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29 + 128) >> 8) as u8
}

fn from_pixels(width: u32, height: u32, data: &[u8], channels: usize) -> Result<Bitmap, ImageError> {
    if width as u64 * height as u64 > MAX_PIXELS {
        return Err(ImageError::TooLarge);
    }
    let mut out = Bitmap::new(width, height, 255);
    let row_bytes = width as usize * channels;
    for y in 0..height as usize {
        let src = &data[y * row_bytes..(y + 1) * row_bytes];
        let dst = &mut out.row_mut(y as u32)[..width as usize];
        match channels {
            1 => dst.copy_from_slice(src),
            // 灰度 + alpha: 合成到白底
            2 => {
                for (d, px) in dst.iter_mut().zip(src.chunks_exact(2)) {
                    let a = px[1] as u32;
                    *d = ((px[0] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                }
            }
            3 => {
                for (d, px) in dst.iter_mut().zip(src.chunks_exact(3)) {
                    *d = luma(px[0], px[1], px[2]);
                }
            }
            4 => {
                for (d, px) in dst.iter_mut().zip(src.chunks_exact(4)) {
                    let a = px[3] as u32;
                    let l = luma(px[0], px[1], px[2]) as u32;
                    *d = ((l * a + 255 * (255 - a) + 127) / 255) as u8;
                }
            }
            _ => return Err(ImageError::Unsupported),
        }
    }
    Ok(out)
}

/// JPEG 的三个分量是否是 YCbCr (与 jpeg-decoder 的判断一致: Adobe 标记的 transform 优先,
/// 否则分量 id 为 'R','G','B' 时是 RGB, 其余按 JFIF 的 YCbCr)。只扫描到 SOS 之前的标记段。
fn jpeg_is_ycbcr(bytes: &[u8]) -> bool {
    let mut adobe_transform = None;
    let mut components: Option<Vec<u8>> = None;
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return false;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let Some(data) = bytes.get(i + 4..i + 2 + len) else { return false };
        match marker {
            0xEE if data.len() >= 12 && data.starts_with(b"Adobe") => adobe_transform = Some(data[11]),
            0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC && data.len() >= 6 => {
                let n = data[5] as usize;
                components = Some((0..n).filter_map(|k| data.get(6 + 3 * k).copied()).collect());
            }
            _ => {}
        }
        i += 2 + len;
    }
    match components {
        Some(ids) if ids.len() == 3 => match adobe_transform {
            Some(t) => t != 0,
            None => ids != b"RGB",
        },
        _ => false,
    }
}

fn decode_jpeg(bytes: &[u8], target: Option<(u32, u32)>) -> Result<Bitmap, ImageError> {
    decode_jpeg_with(bytes, target, true)
}

fn decode_jpeg_with(bytes: &[u8], target: Option<(u32, u32)>, luma_only: bool) -> Result<Bitmap, ImageError> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    let ycbcr = luma_only && jpeg_is_ycbcr(bytes);
    if ycbcr {
        // 不做 YCbCr → RGB: "RGB" 变换对三个分量只做交错, 输出就是 Y, Cb, Cr。
        // (ColorTransform::None 在 jpeg-decoder 0.3.2 里不交错多分量数据, 会越界 panic)
        decoder.set_color_transform(jpeg_decoder::ColorTransform::RGB);
    }
    decoder.read_info().map_err(|e| ImageError::Decode(e.to_string()))?;
    if let Some((w, h)) = target {
        // 解码器会选 "不小于请求尺寸" 的最小缩放档
        let w = w.clamp(1, u16::MAX as u32) as u16;
        let h = h.clamp(1, u16::MAX as u32) as u16;
        decoder.scale(w, h).map_err(|e| ImageError::Decode(e.to_string()))?;
    }
    let pixels = decoder.decode().map_err(|e| ImageError::Decode(e.to_string()))?;
    let info = decoder.info().ok_or_else(|| ImageError::Decode("缺少图片信息".into()))?;
    let channels = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        // CMYK 极少见, 按不支持处理
        _ => return Err(ImageError::Unsupported),
    };
    if ycbcr && channels == 3 {
        // 未做颜色转换: 每像素 Y, Cb, Cr, 取 Y
        return from_plane(info.width as u32, info.height as u32, &pixels, info.width as usize * 3, 3, None);
    }
    from_pixels(info.width as u32, info.height as u32, &pixels, channels)
}

/// 从单个平面 (`step` 字节取一个) 建灰度位图, 可选查表映射。
fn from_plane(width: u32, height: u32, data: &[u8], stride: usize, step: usize, lut: Option<&[u8; 256]>) -> Result<Bitmap, ImageError> {
    if width as u64 * height as u64 > MAX_PIXELS {
        return Err(ImageError::TooLarge);
    }
    let mut out = Bitmap::new(width, height, 255);
    for y in 0..height as usize {
        let src = &data[y * stride..];
        let dst = &mut out.row_mut(y as u32)[..width as usize];
        match (step, lut) {
            (1, None) => dst.copy_from_slice(&src[..width as usize]),
            (1, Some(lut)) => {
                for (d, &v) in dst.iter_mut().zip(&src[..width as usize]) {
                    *d = lut[v as usize];
                }
            }
            (_, None) => {
                for (d, px) in dst.iter_mut().zip(src.chunks(step)) {
                    *d = px[0];
                }
            }
            (_, Some(lut)) => {
                for (d, px) in dst.iter_mut().zip(src.chunks(step)) {
                    *d = lut[px[0] as usize];
                }
            }
        }
    }
    Ok(out)
}

/// VP8 窄范围亮度 → 全范围灰度 (image-webp 的 YUV → RGB 用同一系数: 19077 / 2^14 ≈ 1.164)。
fn vp8_luma_lut() -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (y, v) in lut.iter_mut().enumerate() {
        *v = (((y as i32 - 16) * 19077 + (1 << 13)) >> 14).clamp(0, 255) as u8;
    }
    lut
}

/// 有损、无透明、非动图的 WebP 里 `VP8 ` 块的数据; 其它情况 None (走完整解码)。
fn webp_plain_vp8(bytes: &[u8]) -> Option<&[u8]> {
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let fourcc = &bytes[i..i + 4];
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        let data = bytes.get(i + 8..i + 8 + size)?;
        match fourcc {
            b"VP8 " => return Some(data),
            // 扩展格式: 有透明 (0x10) 或动图 (0x02) 就不走快速路径
            b"VP8X" if data.first().is_some_and(|f| f & 0x12 != 0) => return None,
            b"VP8L" | b"ALPH" | b"ANIM" => return None,
            _ => {}
        }
        i += 8 + size + (size & 1);
    }
    None
}

fn decode_png(bytes: &[u8]) -> Result<Bitmap, ImageError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    // 调色板/16 位/透明度统一展开成 8 位
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| ImageError::Decode(e.to_string()))?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(ImageError::TooLarge);
    }
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(|e| ImageError::Decode(e.to_string()))?;
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(ImageError::Unsupported),
    };
    from_pixels(frame.width, frame.height, &buf[..frame.buffer_size()], channels)
}

fn decode_webp(bytes: &[u8]) -> Result<Bitmap, ImageError> {
    decode_webp_with(bytes, true)
}

fn decode_webp_with(bytes: &[u8], luma_only: bool) -> Result<Bitmap, ImageError> {
    if let Some(vp8) = webp_plain_vp8(bytes).filter(|_| luma_only) {
        let frame = image_webp::vp8::Vp8Decoder::decode_frame(Cursor::new(vp8)).map_err(|e| ImageError::Decode(e.to_string()))?;
        let (w, h) = (frame.width as u32, frame.height as u32);
        // 亮度平面按宏块 (16) 对齐
        let stride = frame.ybuf.len() / (h.div_ceil(16) as usize * 16).max(1);
        if w == 0 || h == 0 || stride < w as usize {
            return Err(ImageError::Decode("VP8 帧尺寸异常".into()));
        }
        return from_plane(w, h, &frame.ybuf, stride, 1, Some(&vp8_luma_lut()));
    }
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| ImageError::Decode(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(ImageError::TooLarge);
    }
    let channels = if decoder.has_alpha() { 4 } else { 3 };
    let size = decoder.output_buffer_size().ok_or(ImageError::TooLarge)?;
    let mut buf = vec![0; size];
    // 动图只取第一帧
    decoder.read_image(&mut buf).map_err(|e| ImageError::Decode(e.to_string()))?;
    from_pixels(w, h, &buf, channels)
}

/// 解码为灰度位图。`target` 给出最终要显示的大致尺寸时, JPEG 会做 DCT 缩放 (结果不小于 target);
/// 调用方再按需 `fit_within`。
pub fn decode_gray(bytes: &[u8], target: Option<(u32, u32)>) -> Result<Bitmap, ImageError> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg(bytes, target)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        decode_webp(bytes)
    } else {
        Err(ImageError::Unsupported)
    }
}

/// 图片高度档位 (与 Python 版 `height_bucket` 一致, 用作 CDN 缩放参数与缓存键)。
pub const HEIGHT_BUCKETS: [u32; 7] = [256, 384, 512, 768, 1024, 1536, 2048];

pub fn height_bucket(height: u32) -> u32 {
    HEIGHT_BUCKETS.iter().copied().find(|&b| height <= b).unwrap_or(HEIGHT_BUCKETS[HEIGHT_BUCKETS.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_match_python() {
        assert_eq!(height_bucket(1), 256);
        assert_eq!(height_bucket(256), 256);
        assert_eq!(height_bucket(257), 384);
        assert_eq!(height_bucket(900), 1024);
        assert_eq!(height_bucket(5000), 2048);
    }

    #[test]
    fn rgba_composites_onto_white() {
        // 1×2: 不透明黑, 全透明黑 → 0, 255
        let data = [0, 0, 0, 255, 0, 0, 0, 0];
        let b = from_pixels(1, 2, &data, 4).unwrap();
        assert_eq!((b.get(0, 0), b.get(0, 1)), (0, 255));
    }

    #[test]
    fn png_round_trip() {
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, 3, 2);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128]).unwrap();
        }
        let b = decode_gray(&bytes, None).unwrap();
        assert_eq!((b.width(), b.height()), (3, 2));
        assert_eq!(b.get(0, 0), 255);
        assert_eq!(b.get(1, 0), 0);
        assert_eq!(b.get(2, 1), 128);
    }

    #[test]
    fn webp_round_trip() {
        // 2x1 RGB 无损 WebP: 红、白
        let mut bytes = Vec::new();
        image_webp::WebPEncoder::new(&mut bytes)
            .encode(&[255, 0, 0, 255, 255, 255], 2, 1, image_webp::ColorType::Rgb8)
            .unwrap();
        let bmp = decode_gray(&bytes, None).unwrap();
        assert_eq!((bmp.width(), bmp.height()), (2, 1));
        assert_eq!(bmp.get(0, 0), luma(255, 0, 0));
        assert_eq!(bmp.get(1, 0), 255);
    }

    /// 只取亮度的结果与完整解码 (RGB → 灰度) 相差很小。
    /// 测试图是高饱和的彩色渐变: 完整路径里 R/G/B 各自截断到 0..255 后再算灰度, 与直接取亮度
    /// 会有几个灰阶的差 (黑白漫画几乎没有); 平均差必须很小。
    fn assert_close(a: &Bitmap, b: &Bitmap, max: u8) {
        assert_eq!((a.width(), a.height()), (b.width(), b.height()));
        let (mut worst, mut sum, mut n) = (0u8, 0u64, 0u64);
        for y in 0..a.height() {
            for (p, q) in a.row(y)[..a.width() as usize].iter().zip(&b.row(y)[..b.width() as usize]) {
                worst = worst.max(p.abs_diff(*q));
                sum += p.abs_diff(*q) as u64;
                n += 1;
            }
        }
        let mean = sum as f64 / n as f64;
        assert!(worst <= max && mean <= 1.5, "max diff {worst}, mean {mean:.2}");
    }

    #[test]
    fn jpeg_luma_matches_rgb_path() {
        let bytes = include_bytes!("../tests/data/color.jpg");
        assert!(jpeg_is_ycbcr(bytes));
        let fast = decode_jpeg_with(bytes, None, true).unwrap();
        let full = decode_jpeg_with(bytes, None, false).unwrap();
        assert_close(&fast, &full, 8);
        // 灰度 JPEG / 无 SOF: 不是 YCbCr
        assert!(!jpeg_is_ycbcr(&[0xFF, 0xD8, 0xFF, 0xD9]));
    }

    #[test]
    fn jpeg_adobe_rgb_is_not_ycbcr() {
        // SOI, APP14 "Adobe" transform=0, SOF0 三个分量
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xEE, 0, 14];
        b.extend_from_slice(b"Adobe\x00\x64\x00\x00\x00\x00\x00");
        b.extend_from_slice(&[0xFF, 0xC0, 0, 17, 8, 0, 1, 0, 1, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        b.extend_from_slice(&[0xFF, 0xDA]);
        assert!(!jpeg_is_ycbcr(&b));
        let pos = b.iter().position(|&v| v == b'A').unwrap() + 11;
        b[pos] = 1;
        assert!(jpeg_is_ycbcr(&b));
    }

    #[test]
    fn webp_luma_matches_rgb_path() {
        let bytes = include_bytes!("../tests/data/color.webp");
        assert!(webp_plain_vp8(bytes).is_some());
        let fast = decode_webp_with(bytes, true).unwrap();
        let full = decode_webp_with(bytes, false).unwrap();
        assert_close(&fast, &full, 8);
        // 带透明: 不走快速路径 (要合成到白底)
        assert!(webp_plain_vp8(include_bytes!("../tests/data/alpha.webp")).is_none());
        assert!(decode_gray(include_bytes!("../tests/data/alpha.webp"), None).is_ok());
    }

    #[test]
    fn garbage_is_unsupported() {
        assert!(matches!(decode_gray(b"hello", None), Err(ImageError::Unsupported)));
    }
}
