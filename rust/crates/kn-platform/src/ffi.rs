//! Private FFI bindings to the statically-linked FBInk shim (csrc/shim.c).
//! Only compiled for `target_os = "linux"`; see build.rs.
#![allow(dead_code)]

use std::os::raw::{c_int, c_uchar};

#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct ShimState {
    pub view_width: u32,
    pub view_height: u32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub scanline_stride: u32,
    pub bpp: u32,
    pub device_id: u32,
    pub screen_dpi: u32,
    pub current_rota: u8,
    pub is_mtk: u8,
    pub is_sunxi: u8,
    pub is_kindle_legacy: u8,
    pub can_wait_for_submission: u8,
    pub pixel_format: u8,
    pub touch_swap_axes: u8,
    pub touch_mirror_x: u8,
    pub touch_mirror_y: u8,
    pub has_color_panel: u8,
    pub device_name: [u8; 32],
    pub device_codename: [u8; 32],
    pub device_platform: [u8; 32],
}

impl ShimState {
    fn cstr(buf: &[u8; 32]) -> String {
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }
    pub fn device_name_str(&self) -> String {
        Self::cstr(&self.device_name)
    }
    pub fn device_codename_str(&self) -> String {
        Self::cstr(&self.device_codename)
    }
    pub fn device_platform_str(&self) -> String {
        Self::cstr(&self.device_platform)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShimInputDevice {
    pub type_: u32,
    pub fd: i32,
    pub matched: u8,
    pub name: [u8; 96],
    pub path: [u8; 96],
}

impl Default for ShimInputDevice {
    fn default() -> Self {
        ShimInputDevice {
            type_: 0,
            fd: -1,
            matched: 0,
            name: [0; 96],
            path: [0; 96],
        }
    }
}

impl ShimInputDevice {
    fn cstr(buf: &[u8; 96]) -> String {
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }
    pub fn name_str(&self) -> String {
        Self::cstr(&self.name)
    }
    pub fn path_str(&self) -> String {
        Self::cstr(&self.path)
    }
}

unsafe extern "C" {
    pub fn shim_fbink_open() -> c_int;
    pub fn shim_fbink_close(fbfd: c_int) -> c_int;
    pub fn shim_fbink_init(fbfd: c_int, is_verbose: c_int) -> c_int;
    pub fn shim_fbink_reinit(fbfd: c_int) -> c_int;
    pub fn shim_get_state(fbfd: c_int, out: *mut ShimState);
    pub fn shim_get_fb_pointer(fbfd: c_int, buffer_size: *mut usize) -> *mut c_uchar;
    pub fn shim_refresh(
        fbfd: c_int,
        top: u32,
        left: u32,
        width: u32,
        height: u32,
        mode_sel: c_int,
        is_flashing: c_int,
        is_animated: c_int,
    ) -> c_int;
    pub fn shim_get_last_marker() -> u32;
    pub fn shim_wait_for_complete(fbfd: c_int, marker: u32) -> c_int;
    pub fn shim_mtk_set_swipe_data(direction: u8, steps: u8) -> c_int;
    pub fn shim_mtk_toggle_auto_reagl(fbfd: c_int, toggle: c_int) -> c_int;
    pub fn shim_wait_for_any_complete(fbfd: c_int) -> c_int;
    pub fn shim_input_scan(
        match_types: u32,
        exclude_types: u32,
        out: *mut ShimInputDevice,
        max_out: usize,
        scan_only: c_int,
    ) -> usize;
}

// INPUT_DEVICE_TYPE_E bits we use (fbink.h ~1700-1722).
pub const INPUT_TOUCHSCREEN: u32 = 1 << 3;
pub const INPUT_TABLET: u32 = 1 << 5;
pub const INPUT_ROTATION_EVENT: u32 = 1 << 23;
pub const INPUT_KEY: u32 = 1 << 6;
pub const INPUT_POWER_BUTTON: u32 = 1 << 16;
pub const INPUT_PAGINATION_BUTTONS: u32 = 1 << 18;
pub const INPUT_HOME_BUTTON: u32 = 1 << 19;
pub const INPUT_DPAD: u32 = 1 << 22;
pub const INPUT_SCALED_TABLET: u32 = 1 << 24;
pub const INPUT_KINDLE_FRAME_TAP: u32 = 1 << 26;

// Waveform mode selectors matching csrc/shim.c's map_wfm().
// 注意: 以下是 csrc/shim.c `map_wfm` 的选择子编号, 不是 FBInk 的 WFM_MODE_INDEX_T 原值
pub const WFM_AUTO: c_int = 0;
pub const WFM_DU: c_int = 1;
pub const WFM_GC16: c_int = 2;
pub const WFM_GL16: c_int = 3;
pub const WFM_REAGL: c_int = 4;
pub const WFM_A2: c_int = 5;
pub const WFM_REAGLD: c_int = 6;

// MTK_SWIPE_DIRECTION_INDEX_E (fbink.h ~534-542), as consumed by
// fbink_mtk_set_swipe_data / shim_mtk_set_swipe_data. Matches Python
// framebuffer.py's SWIPE_MTK table.
pub const MTK_SWIPE_DIR_DOWN: u8 = 0;
pub const MTK_SWIPE_DIR_UP: u8 = 1;
pub const MTK_SWIPE_DIR_LEFT: u8 = 2;
pub const MTK_SWIPE_DIR_RIGHT: u8 = 3;
