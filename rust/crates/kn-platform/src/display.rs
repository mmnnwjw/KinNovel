//! 显示输出。上层只调用 `Display` trait; 刷新策略 (差分/残影预算/波形选择) 在 kn-ui, 这里只负责执行。

use kn_render::{Bitmap, Rect};

/// 波形。FbinkDisplay 负责映射到 FBInk 的 `WFM_*`; 平台不支持时降级:
/// Reagl → Gl16 → Gc16, A2 → Du → Gc16 (见 rust/research-kindle-devices.md 波形表)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    /// 由驱动自动选择
    Auto,
    /// 全灰阶, 用于界面更新与闪刷
    Gc16,
    /// 灰阶, 不闪, 用于文字较多的局部更新
    Gl16,
    /// 原生阅读器翻页波形 (MTK/新机型), 残影少
    Reagl,
    /// 快速黑白 + 少量灰, 按钮反馈/菜单
    Du,
    /// 最快, 只有黑白, 用于按下反馈
    A2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDir {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RefreshRequest {
    /// 需要刷新的区域 (屏幕坐标, 已裁剪到屏幕)
    pub rect: Rect,
    pub waveform: Waveform,
    /// 全刷 (闪烁黑白) 以清除残影
    pub flash: bool,
    /// MTK 原生翻页动画 (仅 `DeviceInfo::supports_swipe` 时生效, 否则忽略)
    pub swipe: Option<SwipeDir>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceInfo {
    pub width: u32,
    pub height: u32,
    /// 帧缓冲每行字节数 (KPW5 = 1248, 大于宽度)
    pub stride: usize,
    /// 帧缓冲每像素字节数: 1 = 8 位灰度 (绝大多数 Kindle); 3 = RGB24 (ColorSoft 彩屏);
    /// 4 = 32 位。上层始终画 L8, `present` 负责展开。
    pub bytes_per_pixel: usize,
    /// 屏幕 ppi (FBInk deviceQuirks.screenDPI: 167 / 212 / 300)
    pub dpi: u32,
    pub device_name: String,
    pub device_codename: String,
    pub platform: String,
    pub device_id: u32,
    pub is_mtk: bool,
    pub supports_reagl: bool,
    pub supports_swipe: bool,
    /// 触摸坐标变换 (来自 FBInkState)
    pub touch_swap_axes: bool,
    pub touch_mirror_x: bool,
    pub touch_mirror_y: bool,
}

pub trait Display {
    fn info(&self) -> &DeviceInfo;

    /// 把 `frame` 中 `req.rect` 区域写入帧缓冲并提交刷新; 返回刷新 marker。
    /// `frame` 尺寸必须等于屏幕尺寸。只拷贝 rect 内的行段 (按行 memcpy, 考虑 stride)。
    fn present(&mut self, frame: &Bitmap, req: &RefreshRequest) -> std::io::Result<u32>;

    /// 等待某个 marker 完成 (闪刷后再做下一步时用); 超时由实现内部处理。
    fn wait_complete(&mut self, marker: u32);

    /// 屏幕被外部改写过 (锁屏/系统 UI) 后由上层调用, 实现可借此重新初始化 (fbink_reinit)。
    fn reinit(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 主机测试用显示: 内存帧 + 记录所有刷新请求。
pub struct MemoryDisplay {
    info: DeviceInfo,
    pub screen: Bitmap,
    pub requests: Vec<RefreshRequest>,
}

impl MemoryDisplay {
    pub fn new(width: u32, height: u32) -> Self {
        let info = DeviceInfo {
            width,
            height,
            stride: width as usize,
            bytes_per_pixel: 1,
            dpi: 300,
            device_name: "MemoryDisplay".to_string(),
            device_codename: "host".to_string(),
            platform: "host".to_string(),
            device_id: 0,
            is_mtk: false,
            supports_reagl: true,
            supports_swipe: false,
            touch_swap_axes: false,
            touch_mirror_x: false,
            touch_mirror_y: false,
        };
        let screen = Bitmap::new(width, height, 0xFF);
        MemoryDisplay {
            info,
            screen,
            requests: Vec::new(),
        }
    }
}

impl Display for MemoryDisplay {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn present(&mut self, frame: &Bitmap, req: &RefreshRequest) -> std::io::Result<u32> {
        copy_rect_rows(frame, &mut self.screen, req.rect);
        self.requests.push(*req);
        Ok(self.requests.len() as u32)
    }

    fn wait_complete(&mut self, _marker: u32) {}
}

/// 把 `frame` 中 `rect` 区域的每一行拷贝到 `dst` 的同一位置, 按各自 stride 寻址,
/// 不假设 `data().len() == width * height`。裁剪到两者边界的交集。
fn copy_rect_rows(frame: &Bitmap, dst: &mut Bitmap, rect: Rect) {
    let bounds = frame.bounds().intersect(&dst.bounds());
    let Some(bounds) = bounds else { return };
    let Some(rect) = rect.intersect(&bounds) else {
        return;
    };
    if rect.is_empty() {
        return;
    }
    let x0 = rect.x as usize;
    let w = rect.w as usize;
    let src_stride = frame.stride();
    let dst_stride = dst.stride();
    let src_data = frame.data();
    let dst_data = dst.data_mut();
    for y in rect.y..rect.bottom() {
        let y = y as usize;
        let src_off = y * src_stride + x0;
        let dst_off = y * dst_stride + x0;
        if src_off + w > src_data.len() || dst_off + w > dst_data.len() {
            continue;
        }
        dst_data[dst_off..dst_off + w].copy_from_slice(&src_data[src_off..src_off + w]);
    }
}

/// FBInk 实现 (静态链接, 见 build.rs 与 csrc/shim.c, 源自 spike-fbink)。
/// - 打开时 `is_quiet` / 日志走 syslog, 避免 FBInk 往 stdout 打印。
/// - 帧缓冲指针在初始化时获取一次并常驻。
#[cfg(target_os = "linux")]
pub struct FbinkDisplay {
    fbfd: std::os::raw::c_int,
    info: DeviceInfo,
    fb_ptr: *mut u8,
    fb_size: usize,
    fb_stride: usize,
    next_marker: u32,
}

#[cfg(target_os = "linux")]
impl FbinkDisplay {
    pub fn open() -> std::io::Result<Self> {
        use crate::ffi;

        let fbfd = unsafe { ffi::shim_fbink_open() };
        if fbfd < 0 {
            return Err(std::io::Error::from_raw_os_error(-fbfd));
        }
        let rc = unsafe { ffi::shim_fbink_init(fbfd, 0) };
        if rc < 0 {
            unsafe { ffi::shim_fbink_close(fbfd) };
            return Err(std::io::Error::from_raw_os_error(-rc));
        }

        let mut state = ffi::ShimState::default();
        unsafe { ffi::shim_get_state(fbfd, &mut state as *mut ffi::ShimState) };

        let mut fb_size: usize = 0;
        let fb_ptr = unsafe { ffi::shim_get_fb_pointer(fbfd, &mut fb_size as *mut usize) };
        if fb_ptr.is_null() {
            unsafe { ffi::shim_fbink_close(fbfd) };
            return Err(std::io::Error::other("fbink_get_fb_pointer failed"));
        }

        // 只认识 8 位灰度与 24/32 位 RGB (ColorSoft); 其它 (4/16 位, 只有非触屏老机型) 不支持
        let bytes_per_pixel = match state.bpp {
            8 => 1,
            24 => 3,
            32 => 4,
            other => {
                unsafe { ffi::shim_fbink_close(fbfd) };
                return Err(std::io::Error::other(format!("unsupported framebuffer depth: {other} bpp")));
            }
        };

        let is_mtk = state.is_mtk != 0;
        // ASSUMPTION (documented, see research-kindle-devices.md "4. FBInk API
        // 子集与 waveform 映射建议"): FBInk's `FBInkState` does not expose a
        // dedicated `isREAGL` bit, only `is_kindle_legacy` (deviceQuirks
        // isKindleLegacy: device uses the original einkfb EPDC API). KOReader's
        // `device.lua` defaults `isREAGL = yes` for every non-legacy Kindle
        // (including MTK/Bellatrix) and `no` only for legacy einkfb models, so
        // `!is_kindle_legacy` is the best available proxy for REAGL support
        // from FBInk alone. If FBInk ever exposes a direct REAGL/isREAGL flag,
        // switch to that instead of this proxy.
        let supports_reagl = state.is_kindle_legacy == 0;

        let info = DeviceInfo {
            width: state.view_width,
            height: state.view_height,
            stride: state.scanline_stride as usize,
            bytes_per_pixel,
            dpi: if state.screen_dpi > 0 { state.screen_dpi } else { 300 },
            device_name: state.device_name_str(),
            device_codename: state.device_codename_str(),
            platform: state.device_platform_str(),
            device_id: state.device_id,
            is_mtk,
            supports_reagl,
            supports_swipe: is_mtk,
            touch_swap_axes: state.touch_swap_axes != 0,
            touch_mirror_x: state.touch_mirror_x != 0,
            touch_mirror_y: state.touch_mirror_y != 0,
        };

        Ok(FbinkDisplay {
            fbfd,
            info,
            fb_ptr,
            fb_size,
            fb_stride: state.scanline_stride as usize,
            next_marker: 1,
        })
    }

    /// Waveform → FBInk WFM 模式号, 按机型降级 (见 research-kindle-devices.md):
    /// Reagl → Gl16 → Gc16。
    ///
    /// MTK (Bellatrix, PW5+) 机型: FBInk 把 `WFM_REAGL` (=4) 映射到 HWTCON 的
    /// GLR16 (验证见 spike-fbink 实测: REAGL 局部刷新 ~298ms 完成; Python 0.8.0
    /// `framebuffer.py` 同样对 MTK 翻页直接使用 REAGL/GLR16), 所以 MTK 上也
    /// 直接用 `WFM_REAGL`, 不要降级到 GL16。只有在 `!supports_reagl` (legacy
    /// einkfb 机型, 不支持 REAGL) 时才降级到 GL16。
    fn map_waveform(&self, wf: Waveform) -> std::os::raw::c_int {
        use crate::ffi::*;
        match wf {
            Waveform::Auto => WFM_AUTO,
            Waveform::Gc16 => WFM_GC16,
            Waveform::Gl16 => WFM_GL16,
            Waveform::Du => WFM_DU,
            Waveform::A2 => WFM_A2,
            Waveform::Reagl => {
                if self.info.supports_reagl {
                    WFM_REAGL
                } else {
                    WFM_GL16
                }
            }
        }
    }
}

// MTK_SWIPE_DIRECTION_INDEX_E (fbink.h ~534-542): DOWN=0, UP=1, LEFT=2,
// RIGHT=3 (matches Python `framebuffer.py`'s `SWIPE_MTK` table). Duplicated
// as plain constants here (rather than only in the linux-only `ffi` module)
// so `swipe_direction_code` is host-testable without an open `FbinkDisplay`.
#[allow(dead_code)]
const MTK_SWIPE_DIR_DOWN: u8 = 0;
#[allow(dead_code)]
const MTK_SWIPE_DIR_UP: u8 = 1;
const MTK_SWIPE_DIR_LEFT: u8 = 2;
const MTK_SWIPE_DIR_RIGHT: u8 = 3;

/// `SwipeDir` → FBInk `MTK_SWIPE_DIRECTION_INDEX_E`. Only `Left`/`Right` are
/// exposed on `SwipeDir` today (page-turn direction); kept as a free function
/// so it's unit-testable without needing an open `FbinkDisplay`.
fn swipe_direction_code(dir: SwipeDir) -> u8 {
    match dir {
        SwipeDir::Left => MTK_SWIPE_DIR_LEFT,
        SwipeDir::Right => MTK_SWIPE_DIR_RIGHT,
    }
}

#[cfg(target_os = "linux")]
impl Display for FbinkDisplay {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn present(&mut self, frame: &Bitmap, req: &RefreshRequest) -> std::io::Result<u32> {
        use crate::ffi;

        let screen_bounds = Rect::new(0, 0, self.info.width, self.info.height);
        let Some(rect) = req.rect.intersect(&screen_bounds) else {
            return Ok(0);
        };
        if rect.is_empty() {
            return Ok(0);
        }

        // 只拷贝 rect 内的行段到映射的帧缓冲, 考虑 stride (frame 与 fb 的 stride 可能不同)。
        // 8 位直接 memcpy; RGB 帧缓冲 (ColorSoft) 把灰度展开成 R=G=B。
        let x0 = rect.x as usize;
        let w = rect.w as usize;
        let bpp = self.info.bytes_per_pixel;
        let src_stride = frame.stride();
        let src_data = frame.data();
        for y in rect.y..rect.bottom() {
            let y = y as usize;
            let src_off = y * src_stride + x0;
            if src_off + w > src_data.len() {
                continue;
            }
            let dst_off = y * self.fb_stride + x0 * bpp;
            if dst_off + w * bpp > self.fb_size {
                continue;
            }
            let src = &src_data[src_off..src_off + w];
            // SAFETY: dst_off + w*bpp <= fb_size (检查见上), fb_ptr 映射在 Drop 前一直有效
            let dst = unsafe { std::slice::from_raw_parts_mut(self.fb_ptr.add(dst_off), w * bpp) };
            match bpp {
                1 => dst.copy_from_slice(src),
                3 => {
                    for (d, &v) in dst.chunks_exact_mut(3).zip(src) {
                        d.fill(v);
                    }
                }
                _ => {
                    for (d, &v) in dst.chunks_exact_mut(4).zip(src) {
                        d.copy_from_slice(&[v, v, v, 0xFF]);
                    }
                }
            }
        }

        // MTK 原生翻页动画, 仅在设备支持且上层要求时启用。
        let mut is_animated = 0;
        if self.info.supports_swipe {
            if let Some(dir) = req.swipe {
                let direction = swipe_direction_code(dir);
                let rc = unsafe { ffi::shim_mtk_set_swipe_data(direction, 12u8) };
                if rc == 0 {
                    is_animated = 1;
                }
            }
        }

        let wfm = self.map_waveform(req.waveform);
        // i.MX 机型的 REAGL 必须配 UPDATE_MODE_FULL (fbink.c: "REAGL should always be paired with FULL",
        // 由调用方负责; KOReader 同样处理), REAGL 的 FULL 不闪。MTK 上 REAGL + PARTIAL 已实测正常, 保持不变。
        let full = req.flash || (wfm == crate::ffi::WFM_REAGL && !self.info.is_mtk);
        let rc = unsafe {
            ffi::shim_refresh(
                self.fbfd,
                rect.y as u32,
                rect.x as u32,
                rect.w,
                rect.h,
                wfm,
                if full { 1 } else { 0 },
                is_animated,
            )
        };
        if rc < 0 {
            return Err(std::io::Error::from_raw_os_error(-rc));
        }
        // 返回内核实际的 marker, wait_complete 才能真正等到这次刷新完成
        let marker = unsafe { ffi::shim_get_last_marker() };
        self.next_marker = marker;
        Ok(marker)
    }

    fn wait_complete(&mut self, marker: u32) {
        use crate::ffi;
        unsafe {
            ffi::shim_wait_for_complete(self.fbfd, marker);
        }
    }

    fn reinit(&mut self) -> std::io::Result<()> {
        use crate::ffi;
        let rc = unsafe { ffi::shim_fbink_reinit(self.fbfd) };
        if rc < 0 {
            return Err(std::io::Error::from_raw_os_error(-rc));
        }
        let mut state = ffi::ShimState::default();
        unsafe { ffi::shim_get_state(self.fbfd, &mut state as *mut ffi::ShimState) };
        let mut fb_size: usize = 0;
        let fb_ptr = unsafe { ffi::shim_get_fb_pointer(self.fbfd, &mut fb_size as *mut usize) };
        if !fb_ptr.is_null() {
            self.fb_ptr = fb_ptr;
            self.fb_size = fb_size;
            self.fb_stride = state.scanline_stride as usize;
            self.info.width = state.view_width;
            self.info.height = state.view_height;
            self.info.stride = state.scanline_stride as usize;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for FbinkDisplay {
    fn drop(&mut self) {
        use crate::ffi;
        unsafe {
            ffi::shim_fbink_close(self.fbfd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_display_records_requests_and_copies_rect() {
        let mut d = MemoryDisplay::new(100, 50);
        let frame = Bitmap::new(100, 50, 0x00);
        let req = RefreshRequest {
            rect: Rect::new(10, 10, 20, 20),
            waveform: Waveform::Gc16,
            flash: false,
            swipe: None,
        };
        d.present(&frame, &req).unwrap();
        assert_eq!(d.requests.len(), 1);
        assert_eq!(d.requests[0].rect, req.rect);
        // outside the rect, screen should remain untouched (still 0xFF from new()).
        assert_eq!(d.screen.get(0, 0), 0xFF);
        // inside the rect, should now be 0x00 (copied from the all-black frame).
        assert_eq!(d.screen.get(15, 15), 0x00);
    }

    #[test]
    fn swipe_direction_maps_to_mtk_indices() {
        // MTK_SWIPE_DIRECTION_INDEX_E (fbink.h): DOWN=0, UP=1, LEFT=2, RIGHT=3.
        assert_eq!(swipe_direction_code(SwipeDir::Left), 2);
        assert_eq!(swipe_direction_code(SwipeDir::Right), 3);
    }
}
