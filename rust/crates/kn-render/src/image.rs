//! 图片解码: JPEG / PNG → L8 灰度 `Bitmap` (墨水屏只需要灰度)。
//!
//! JPEG 用 DCT 缩放解码 (1/2、1/4、1/8), 只解到 "不小于目标尺寸" 的最小档 —— 1200×2234 的插图
//! 放进 ~800 px 高的区域时只解 1/2, 设备上省下大半时间 (对应 Python 版 Pillow 的 `draft()`)。
//! 之后再用 `Bitmap::fit_within` 区域平均缩到最终尺寸。

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

fn decode_jpeg(bytes: &[u8], target: Option<(u32, u32)>) -> Result<Bitmap, ImageError> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
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
    from_pixels(info.width as u32, info.height as u32, &pixels, channels)
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

/// 解码为灰度位图。`target` 给出最终要显示的大致尺寸时, JPEG 会做 DCT 缩放 (结果不小于 target);
/// 调用方再按需 `fit_within`。
pub fn decode_gray(bytes: &[u8], target: Option<(u32, u32)>) -> Result<Bitmap, ImageError> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg(bytes, target)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)
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
    fn garbage_is_unsupported() {
        assert!(matches!(decode_gray(b"hello", None), Err(ImageError::Unsupported)));
    }
}
