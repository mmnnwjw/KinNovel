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

/// 原图最多 32 M 像素 (灰度位图 32 MB)。解析完文件头、分配任何像素缓冲之前就按原始尺寸检查。
const MAX_PIXELS: u64 = 32 * 1024 * 1024;

/// 解码器自己的每块大缓冲 (多通道输出、渐进式 JPEG 的系数、PNG/WebP 的 RGB(A) 帧) 最多 64 MB。
/// 加上最终的灰度位图, 峰值约 100 MB, 256 MB 的老机型 (框架已暂停) 也扛得住;
/// 正常插图/漫画 (CDN 最高 2048 档, 原图通常 < 10 M 像素) 远在线内。
/// 折算: RGB 约 21 M 像素, RGBA 16 M 像素, 4:2:0 渐进式 JPEG 约 21 M 像素。
const MAX_BUFFER: u64 = 64 * 1024 * 1024;

/// 仅用于用户在全屏预览中明确选择“仍要加载”的一次解码。
/// 提高单缓冲额度, 原始像素硬上限保持不变; 不影响封面、漫画和其它并行任务。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageBudget {
    #[default]
    Normal,
    LargeOriginal,
}

impl ImageBudget {
    fn buffer_bytes(self) -> u64 {
        match self {
            Self::Normal => MAX_BUFFER,
            Self::LargeOriginal => 96 * 1024 * 1024,
        }
    }
}

/// RGB → 灰度 (ITU-R BT.601, 整数近似)。
#[inline]
fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29 + 128) >> 8) as u8
}

#[cfg_attr(kn_libwebp, allow(dead_code))]
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

/// JPEG 文件头里我们关心的部分: Adobe transform 与 SOF 的分量 (id, 水平采样, 垂直采样)。
#[derive(Default)]
struct JpegHeader {
    adobe_transform: Option<u8>,
    components: Option<Vec<(u8, u8, u8)>>,
}

impl JpegHeader {
    /// 三个分量是否是 YCbCr (与 jpeg-decoder 的判断一致: Adobe 标记的 transform 优先,
    /// 否则分量 id 为 'R','G','B' 时是 RGB, 其余按 JFIF 的 YCbCr)。
    fn is_ycbcr(&self) -> bool {
        match &self.components {
            Some(c) if c.len() == 3 => match self.adobe_transform {
                Some(t) => t != 0,
                None => c.iter().map(|x| x.0).ne(*b"RGB"),
            },
            _ => false,
        }
    }

    /// 渐进式解码时 jpeg-decoder 按原始尺寸给每个分量分配的 DCT 系数 (每块 64 个 i16) 字节数。
    /// 解析不出采样因子时按每个分量全分辨率估算。
    fn coefficient_bytes(&self, width: u64, height: u64, channels: u64) -> u64 {
        let blocks = |n: u64| n.div_ceil(8);
        match &self.components {
            Some(c) if !c.is_empty() && c.iter().all(|x| x.1 > 0 && x.2 > 0) => {
                let hmax = c.iter().map(|x| x.1 as u64).max().unwrap_or(1);
                let vmax = c.iter().map(|x| x.2 as u64).max().unwrap_or(1);
                c.iter()
                    .map(|&(_, h, v)| blocks((width * h as u64).div_ceil(hmax)) * blocks((height * v as u64).div_ceil(vmax)) * 128)
                    .sum()
            }
            _ => blocks(width) * blocks(height) * 128 * channels,
        }
    }
}

/// 扫描 SOS 之前的标记段。
fn jpeg_header(bytes: &[u8]) -> JpegHeader {
    let mut header = JpegHeader::default();
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return JpegHeader::default();
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
        let Some(data) = bytes.get(i + 4..i + 2 + len) else { return JpegHeader::default() };
        match marker {
            0xEE if data.len() >= 12 && data.starts_with(b"Adobe") => header.adobe_transform = Some(data[11]),
            0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC && data.len() >= 6 => {
                let n = data[5] as usize;
                let comps = (0..n).filter_map(|k| data.get(6 + 3 * k..8 + 3 * k)).map(|c| (c[0], c[1] >> 4, c[1] & 15));
                header.components = Some(comps.collect());
            }
            _ => {}
        }
        i += 2 + len;
    }
    header
}

fn decode_jpeg_with(bytes: &[u8], target: Option<(u32, u32)>, luma_only: bool, max_buffer: u64) -> Result<Bitmap, ImageError> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    let header = jpeg_header(bytes);
    let ycbcr = luma_only && header.is_ycbcr();
    if ycbcr {
        // 不做 YCbCr → RGB: "RGB" 变换对三个分量只做交错, 输出就是 Y, Cb, Cr。
        // (ColorTransform::None 在 jpeg-decoder 0.3.2 里不交错多分量数据, 会越界 panic)
        decoder.set_color_transform(jpeg_decoder::ColorTransform::RGB);
    }
    decoder.read_info().map_err(|e| ImageError::Decode(e.to_string()))?;
    // 先按原始尺寸检查: SOF 可以声明 65535×65535, 不查的话要解几十秒、分配上 GB 才报错,
    // 32 位的 Kindle 早就 OOM 了 (jpeg-decoder 自带的缓冲上限要等所有扫描段解完才检查)。
    let info = decoder.info().ok_or_else(|| ImageError::Decode("缺少图片信息".into()))?;
    let channels = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => 1,
        jpeg_decoder::PixelFormat::RGB24 => 3,
        // CMYK / 16 位极少见, 按不支持处理
        _ => return Err(ImageError::Unsupported),
    };
    let (w0, h0) = (info.width as u64, info.height as u64);
    if w0 * h0 > MAX_PIXELS {
        return Err(ImageError::TooLarge);
    }
    // 渐进式的系数按原始尺寸存 (DCT 缩放省不掉), 第一个扫描段就整块分配
    if info.coding_process == jpeg_decoder::CodingProcess::DctProgressive && header.coefficient_bytes(w0, h0, channels) > max_buffer {
        return Err(ImageError::TooLarge);
    }
    if let Some((w, h)) = target {
        // 解码器会选 "不小于请求尺寸" 的最小缩放档
        let w = w.clamp(1, u16::MAX as u32) as u16;
        let h = h.clamp(1, u16::MAX as u32) as u16;
        decoder.scale(w, h).map_err(|e| ImageError::Decode(e.to_string()))?;
    }
    // 缩放后的多通道输出缓冲; 同一上限也交给解码器兜底
    let out = decoder.info().ok_or_else(|| ImageError::Decode("缺少图片信息".into()))?;
    if out.width as u64 * out.height as u64 * channels > max_buffer {
        return Err(ImageError::TooLarge);
    }
    decoder.set_max_decoding_buffer_size(max_buffer as usize);
    let pixels = decoder.decode().map_err(|e| ImageError::Decode(e.to_string()))?;
    let info = decoder.info().ok_or_else(|| ImageError::Decode("缺少图片信息".into()))?;
    if ycbcr && channels == 3 {
        // 未做颜色转换: 每像素 Y, Cb, Cr, 取 Y
        return from_plane(info.width as u32, info.height as u32, &pixels, info.width as usize * 3, 3, None);
    }
    from_pixels(info.width as u32, info.height as u32, &pixels, channels as usize)
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

/// `VP8 ` 关键帧头里的尺寸 (14 位, 最大 16383×16383); 不是关键帧或头不完整时 None。
#[cfg_attr(kn_libwebp, allow(dead_code))]
fn vp8_dimensions(vp8: &[u8]) -> Option<(u32, u32)> {
    if vp8.len() < 10 || vp8[0] & 1 != 0 || vp8[3..6] != [0x9d, 0x01, 0x2a] {
        return None;
    }
    let w = u16::from_le_bytes([vp8[6], vp8[7]]) & 0x3FFF;
    let h = u16::from_le_bytes([vp8[8], vp8[9]]) & 0x3FFF;
    Some((w as u32, h as u32))
}

/// 有损、无透明、非动图的 WebP 里 `VP8 ` 块的数据; 其它情况 None (走完整解码)。
#[cfg_attr(kn_libwebp, allow(dead_code))]
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

fn decode_png(bytes: &[u8], max_buffer: u64) -> Result<Bitmap, ImageError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    // 调色板/16 位/透明度统一展开成 8 位
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| ImageError::Decode(e.to_string()))?;
    let (w, h) = (reader.info().width, reader.info().height);
    // 展开后的 RGB(A) 缓冲也设限 (RGBA 每像素 4 字节, 32 M 像素就要 128 MB)
    if w as u64 * h as u64 > MAX_PIXELS || reader.output_buffer_size() as u64 > max_buffer {
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

/// WebP: 设备 (Linux) 上用静态编入的 libwebp (NEON, 直接解码到目标尺寸), 其它平台用 image-webp。
fn decode_webp(bytes: &[u8], target: Option<(u32, u32)>, max_buffer: u64) -> Result<Bitmap, ImageError> {
    #[cfg(kn_libwebp)]
    {
        decode_webp_native(bytes, target, max_buffer)
    }
    #[cfg(not(kn_libwebp))]
    {
        let _ = target;
        decode_webp_with(bytes, true, max_buffer)
    }
}

#[cfg(kn_libwebp)]
mod native {
    extern "C" {
        pub fn kn_webp_decode_luma(data: *const u8, size: usize, tw: i32, th: i32, max_pixels: u64, max_bytes: u64, ow: *mut i32, oh: *mut i32, status: *mut i32) -> *mut u8;
        pub fn kn_webp_free(p: *mut u8);
    }
}

/// libwebp 解码亮度平面 (`target` 给出时用它的缩放器直接解码到等比放进 target 的尺寸)。
#[cfg(kn_libwebp)]
fn decode_webp_native(bytes: &[u8], target: Option<(u32, u32)>, max_buffer: u64) -> Result<Bitmap, ImageError> {
    let (tw, th) = target.map_or((0, 0), |(w, h)| (w.min(i32::MAX as u32) as i32, h.min(i32::MAX as u32) as i32));
    let (mut w, mut h, mut status) = (0i32, 0i32, 0i32);
    // SAFETY: bytes 在调用期间有效; 返回的缓冲区 (w*h 字节) 由 kn_webp_free 释放, 只在本函数内读取
    let ptr = unsafe { native::kn_webp_decode_luma(bytes.as_ptr(), bytes.len(), tw, th, MAX_PIXELS, max_buffer, &mut w, &mut h, &mut status) };
    if ptr.is_null() {
        return Err(match status {
            -1 => ImageError::TooLarge,
            s => ImageError::Decode(format!("libwebp 解码失败 ({s})")),
        });
    }
    let (w, h) = (w as u32, h as u32);
    let plane = unsafe { std::slice::from_raw_parts(ptr, w as usize * h as usize) };
    let result = from_plane(w, h, plane, w as usize, 1, Some(&vp8_luma_lut()));
    unsafe { native::kn_webp_free(ptr) };
    result
}

#[cfg_attr(kn_libwebp, allow(dead_code))]
fn decode_webp_with(bytes: &[u8], luma_only: bool, max_buffer: u64) -> Result<Bitmap, ImageError> {
    if let Some(vp8) = webp_plain_vp8(bytes).filter(|_| luma_only) {
        // image-webp 读完帧头就按宏块对齐分配 Y/U/V 平面, 先自己看尺寸
        if vp8_dimensions(vp8).is_some_and(|(w, h)| (w.div_ceil(16) * 16) as u64 * (h.div_ceil(16) * 16) as u64 > MAX_PIXELS) {
            return Err(ImageError::TooLarge);
        }
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
    decoder.set_memory_limit(max_buffer as usize);
    let (w, h) = decoder.dimensions();
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(ImageError::TooLarge);
    }
    let channels = if decoder.has_alpha() { 4 } else { 3 };
    // 解码器内部还有一块同样大小的帧 (有损: RGB, 无损/动图: RGBA), 按输出缓冲设限
    let size = decoder.output_buffer_size().filter(|&n| n as u64 <= max_buffer).ok_or(ImageError::TooLarge)?;
    let mut buf = vec![0; size];
    // 动图只取第一帧
    decoder.read_image(&mut buf).map_err(|e| ImageError::Decode(e.to_string()))?;
    from_pixels(w, h, &buf, channels)
}

/// 解码为灰度位图。`target` 给出最终要显示的大致尺寸时, JPEG 会做 DCT 缩放 (结果不小于 target);
/// 调用方再按需 `fit_within`。
pub fn decode_gray(bytes: &[u8], target: Option<(u32, u32)>) -> Result<Bitmap, ImageError> {
    decode_gray_with_budget(bytes, target, ImageBudget::Normal)
}

/// 按本次请求的额度解码, 不修改全局限制。
pub fn decode_gray_with_budget(bytes: &[u8], target: Option<(u32, u32)>, budget: ImageBudget) -> Result<Bitmap, ImageError> {
    let max_buffer = budget.buffer_bytes();
    if bytes.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg_with(bytes, target, true, max_buffer)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes, max_buffer)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        decode_webp(bytes, target, max_buffer)
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
        let fast = decode_jpeg_with(bytes, None, true, MAX_BUFFER).unwrap();
        let full = decode_jpeg_with(bytes, None, false, MAX_BUFFER).unwrap();
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
        let fast = decode_webp_with(bytes, true, MAX_BUFFER).unwrap();
        let full = decode_webp_with(bytes, false, MAX_BUFFER).unwrap();
        assert_close(&fast, &full, 8);
        // 带透明: 不走快速路径 (要合成到白底)
        assert!(webp_plain_vp8(include_bytes!("../tests/data/alpha.webp")).is_none());
        assert!(decode_gray(include_bytes!("../tests/data/alpha.webp"), None).is_ok());
    }

    /// libwebp 与 image-webp 的亮度平面一致 (VP8 解码是规范精确的), 缩放解码等比放进目标尺寸。
    #[cfg(kn_libwebp)]
    #[test]
    fn libwebp_matches_image_webp_luma() {
        let bytes = include_bytes!("../tests/data/color.webp");
        let native = decode_webp_native(bytes, None, MAX_BUFFER).unwrap();
        let reference = decode_webp_with(bytes, true, MAX_BUFFER).unwrap();
        assert_eq!((native.width(), native.height()), (reference.width(), reference.height()));
        let max_diff = (0..native.height())
            .flat_map(|y| (0..native.width()).map(move |x| (x, y)))
            .map(|(x, y)| (native.get(x, y) as i32 - reference.get(x, y) as i32).abs())
            .max()
            .unwrap();
        assert!(max_diff <= 1, "max diff {max_diff}");
        let (w, h) = (native.width() / 3, native.height() / 3);
        let scaled = decode_webp_native(bytes, Some((w, h)), MAX_BUFFER).unwrap();
        assert!(scaled.width() <= w && scaled.height() <= h && (scaled.width() == w || scaled.height() == h));
        assert!(decode_webp_native(include_bytes!("../tests/data/alpha.webp"), Some((8, 8)), MAX_BUFFER).is_ok());
        assert!(decode_webp_native(b"RIFF\0\0\0\0WEBPVP8 junk", None, MAX_BUFFER).is_err());
    }

    fn jpeg_is_ycbcr(bytes: &[u8]) -> bool {
        jpeg_header(bytes).is_ycbcr()
    }

    /// 把测试 JPEG 的 SOF 改成 `w`×`h` (`sof` = 0xC0 基线 / 0xC2 渐进式)。
    fn jpeg_with_size(w: u16, h: u16, sof: u8) -> Vec<u8> {
        let mut b = include_bytes!("../tests/data/color.jpg").to_vec();
        let i = b.windows(2).position(|m| m == [0xFF, 0xC0]).unwrap();
        b[i + 1] = sof;
        b[i + 5..i + 7].copy_from_slice(&h.to_be_bytes());
        b[i + 7..i + 9].copy_from_slice(&w.to_be_bytes());
        b
    }

    /// 像素、RGB 输出与渐进式系数的代表性预算边界, 不运行大图解码压力测试。
    #[test]
    fn jpeg_limits_are_checked_before_decoding() {
        let bytes = jpeg_with_size(6000, 6000, 0xC0);
        for budget in [ImageBudget::Normal, ImageBudget::LargeOriginal] {
            assert!(matches!(decode_gray_with_budget(&bytes, Some((800, 1000)), budget), Err(ImageError::TooLarge)));
        }
        for (sof, target) in [(0xC0, None), (0xC2, Some((800, 1000)))] {
            let bytes = jpeg_with_size(6000, 5000, sof);
            assert!(matches!(decode_gray(&bytes, target), Err(ImageError::TooLarge)));
        }
        // 系数估算: 4:2:0 每像素 1.5 个系数 × 2 字节
        let header = jpeg_header(&jpeg_with_size(6000, 5000, 0xC2));
        assert_eq!(header.coefficient_bytes(6000, 5000, 3), 750 * 625 * 128 + 2 * 375 * 313 * 128);
    }

    /// 正常尺寸的 JPEG 照旧解码, 结果与直接用 jpeg-decoder 取 Y 一致。
    #[test]
    fn normal_jpeg_still_decodes() {
        let bytes = include_bytes!("../tests/data/color.jpg");
        let mut d = jpeg_decoder::Decoder::new(Cursor::new(&bytes[..]));
        d.set_color_transform(jpeg_decoder::ColorTransform::RGB);
        let raw = d.decode().unwrap();
        let ours = decode_gray(bytes, None).unwrap();
        assert_eq!((ours.width(), ours.height()), (96, 64));
        for y in 0..64u32 {
            for x in 0..96u32 {
                assert_eq!(ours.get(x, y), raw[(y as usize * 96 + x as usize) * 3]);
            }
        }
        let scaled = decode_gray(bytes, Some((24, 16))).unwrap();
        assert_eq!((scaled.width(), scaled.height()), (24, 16));
        // 渐进式标记 + 原尺寸: 通过尺寸检查 (数据不是真的渐进式, 解码失败也不应是 TooLarge)
        assert!(!matches!(decode_gray(&jpeg_with_size(96, 64, 0xC2), None), Err(ImageError::TooLarge)));
    }

    #[test]
    fn png_and_webp_limits_are_checked_before_decoding() {
        // PNG: 只有 IHDR 声明 4200×4200 RGBA (略超默认 64 MiB 缓冲)
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend_from_slice(&4200u32.to_be_bytes());
        ihdr.extend_from_slice(&4200u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(&ihdr);
        png.extend_from_slice(&png_crc(&ihdr).to_be_bytes());
        // 空 IDAT: read_info 读到图像数据开头为止
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(b"IDAT");
        png.extend_from_slice(&png_crc(b"IDAT").to_be_bytes());
        assert!(matches!(decode_gray(&png, None), Err(ImageError::TooLarge)));

        // WebP: 6000×6000 超过像素上限, 只检查帧头而不分配图像缓冲
        let mut webp = include_bytes!("../tests/data/color.webp").to_vec();
        let i = webp.windows(4).position(|m| m == b"VP8 ").unwrap() + 8;
        assert_eq!(vp8_dimensions(&webp[i..]).map(|(w, h)| w * h > 0), Some(true));
        webp[i + 6..i + 8].copy_from_slice(&6000u16.to_le_bytes());
        webp[i + 8..i + 10].copy_from_slice(&6000u16.to_le_bytes());
        assert!(matches!(decode_gray(&webp, None), Err(ImageError::TooLarge)));
    }

    fn png_crc(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    #[test]
    fn garbage_is_unsupported() {
        assert!(matches!(decode_gray(b"hello", None), Err(ImageError::Unsupported)));
    }
}
