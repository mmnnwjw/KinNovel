// Minimal C shim around FBInk's public API (ported from rust/spike-fbink/csrc/shim.c).
//
// We deliberately avoid exposing FBInkConfig/FBInkState directly to Rust:
// their layout mixes bools, packed uint8_t enums and multi-byte ints in a way
// that's easy to get subtly wrong across the FFI boundary. Instead we keep a
// single static FBInkConfig here, expose plain-old-data getters/setters, and
// flatten FBInkState into our own simple struct (ShimState) that we control
// on both sides.
#include <string.h>
#include <stdlib.h>
#include "fbink.h"

static FBInkConfig g_cfg;
static FBInkState  g_state;
static bool         g_cfg_init = false;

static void ensure_cfg(void) {
    if (!g_cfg_init) {
        memset(&g_cfg, 0, sizeof(g_cfg));
        // Keep FBInk quiet: it logs to stdout by default which would corrupt
        // our own stdout protocol / clutter the terminal on device.
        g_cfg.is_quiet   = true;
        g_cfg.is_verbose = false;
        g_cfg.to_syslog  = true;
        g_cfg_init = true;
    }
}

int shim_fbink_open(void) {
    return fbink_open();
}

int shim_fbink_close(int fbfd) {
    return fbink_close(fbfd);
}

int shim_fbink_init(int fbfd, int is_verbose) {
    ensure_cfg();
    g_cfg.is_verbose = is_verbose ? true : false;
    g_cfg.is_quiet   = is_verbose ? false : true;
    return fbink_init(fbfd, &g_cfg);
}

int shim_fbink_reinit(int fbfd) {
    ensure_cfg();
    return fbink_reinit(fbfd, &g_cfg);
}

// Flat mirror of the bits of FBInkState we care about for device
// identification + framebuffer geometry. Keep field order/types simple
// (fixed-width ints, fixed-size char buffers) so Rust can #[repr(C)] it
// trivially and correctly.
typedef struct {
    uint32_t view_width;
    uint32_t view_height;
    uint32_t screen_width;
    uint32_t screen_height;
    uint32_t scanline_stride; // bytes per scanline (fInfo.line_length)
    uint32_t bpp;
    uint32_t device_id;
    uint32_t screen_dpi;
    uint8_t  current_rota;
    uint8_t  is_mtk;
    uint8_t  is_sunxi;
    uint8_t  is_kindle_legacy;
    uint8_t  can_wait_for_submission;
    uint8_t  pixel_format;
    uint8_t  touch_swap_axes;
    uint8_t  touch_mirror_x;
    uint8_t  touch_mirror_y;
    uint8_t  has_color_panel;
    char     device_name[32];
    char     device_codename[32];
    char     device_platform[32];
} ShimState;

void shim_get_state(int fbfd, ShimState* out) {
    (void) fbfd;
    fbink_get_state(&g_cfg, &g_state);
    memset(out, 0, sizeof(*out));
    out->view_width               = g_state.view_width;
    out->view_height              = g_state.view_height;
    out->screen_width             = g_state.screen_width;
    out->screen_height            = g_state.screen_height;
    out->scanline_stride          = g_state.scanline_stride;
    out->bpp                      = g_state.bpp;
    out->device_id                = (uint32_t) g_state.device_id;
    out->screen_dpi               = g_state.screen_dpi;
    out->current_rota             = g_state.current_rota;
    out->is_mtk                   = g_state.is_mtk ? 1 : 0;
    out->is_sunxi                 = g_state.is_sunxi ? 1 : 0;
    out->is_kindle_legacy         = g_state.is_kindle_legacy ? 1 : 0;
    out->can_wait_for_submission  = g_state.can_wait_for_submission ? 1 : 0;
    out->pixel_format             = g_state.pixel_format;
    out->touch_swap_axes          = g_state.touch_swap_axes ? 1 : 0;
    out->touch_mirror_x           = g_state.touch_mirror_x ? 1 : 0;
    out->touch_mirror_y           = g_state.touch_mirror_y ? 1 : 0;
    out->has_color_panel          = g_state.has_color_panel ? 1 : 0;
    memcpy(out->device_name, g_state.device_name, sizeof(out->device_name) - 1);
    memcpy(out->device_codename, g_state.device_codename, sizeof(out->device_codename) - 1);
    memcpy(out->device_platform, g_state.device_platform, sizeof(out->device_platform) - 1);
}

unsigned char* shim_get_fb_pointer(int fbfd, size_t* buffer_size) {
    return fbink_get_fb_pointer(fbfd, buffer_size);
}

// mode_sel: 0=AUTO 1=DU 2=GC16 3=GL16 4=REAGL 5=A2 6=REAGLD
static WFM_MODE_INDEX_T map_wfm(int mode_sel) {
    switch (mode_sel) {
        case 1: return WFM_DU;
        case 2: return WFM_GC16;
        case 3: return WFM_GL16;
        case 4: return WFM_REAGL;
        case 5: return WFM_A2;
        case 6: return WFM_REAGLD;
        default: return WFM_AUTO;
    }
}

int shim_refresh(int fbfd, uint32_t top, uint32_t left, uint32_t width, uint32_t height,
                  int mode_sel, int is_flashing, int is_animated) {
    ensure_cfg();
    g_cfg.wfm_mode    = map_wfm(mode_sel);
    g_cfg.is_flashing = is_flashing ? true : false;
    g_cfg.is_animated = is_animated ? true : false;
    return fbink_refresh(fbfd, top, left, width, height, &g_cfg);
}

// fbink_refresh 之后调用, 取内核实际使用的 update marker
uint32_t shim_get_last_marker(void) {
    return fbink_get_last_marker();
}

int shim_wait_for_complete(int fbfd, uint32_t marker) {
    return fbink_wait_for_complete(fbfd, marker);
}

int shim_mtk_set_swipe_data(uint8_t direction, uint8_t steps) {
    return fbink_mtk_set_swipe_data(direction, steps);
}

int shim_mtk_toggle_auto_reagl(int fbfd, int toggle) {
    return fbink_mtk_toggle_auto_reagl(fbfd, toggle ? true : false);
}

int shim_wait_for_any_complete(int fbfd) {
    return fbink_wait_for_any_complete(fbfd);
}

// Flat mirror of FBInkInputDevice, trimmed to fixed-size name/path buffers
// (the real struct carries name[256]/path[4096], which is both overkill and
// awkward to get byte-exact across FFI; we just truncate).
typedef struct {
    uint32_t type;
    int32_t  fd;
    uint8_t  matched;
    char     name[96];
    char     path[96];
} ShimInputDevice;

// Scan input devices via FBInk's own fbink_input_scan (used by KOReader),
// classified with match_types. When `scan_only` is nonzero no fds are left
// open (used just to inspect device names/paths); otherwise matched devices
// are opened O_RDONLY|O_NONBLOCK|O_CLOEXEC and their fd is returned to the
// caller, who then owns it (fbink_input_scan's own heap array is freed here,
// but the fd numbers themselves remain valid after that).
// Returns the number of devices found (may be > max_out; only the first
// max_out are copied into out).
size_t shim_input_scan(uint32_t match_types, uint32_t exclude_types, ShimInputDevice* out, size_t max_out, int scan_only) {
    size_t dev_count = 0;
    INPUT_SETTINGS_TYPE_T settings = (INPUT_SETTINGS_TYPE_T) (NO_RECAP | (scan_only ? SCAN_ONLY : 0));
    FBInkInputDevice* devs = fbink_input_scan(
        (INPUT_DEVICE_TYPE_T) match_types, (INPUT_DEVICE_TYPE_T) exclude_types, settings, &dev_count);
    if (!devs) {
        return 0;
    }
    size_t n = dev_count < max_out ? dev_count : max_out;
    for (size_t i = 0; i < n; i++) {
        out[i].type    = devs[i].type;
        out[i].fd      = devs[i].fd;
        out[i].matched = devs[i].matched ? 1 : 0;
        memset(out[i].name, 0, sizeof(out[i].name));
        memset(out[i].path, 0, sizeof(out[i].path));
        strncpy(out[i].name, devs[i].name, sizeof(out[i].name) - 1);
        strncpy(out[i].path, devs[i].path, sizeof(out[i].path) - 1);
    }
    free(devs);
    return dev_count;
}
