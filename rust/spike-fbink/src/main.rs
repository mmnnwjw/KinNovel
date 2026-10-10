mod ffi;
mod input;

use ffi::*;
use std::time::{Duration, Instant};

const SCREEN_W: usize = 1236;
const SCREEN_H: usize = 1648;

fn main() {
    println!("=== spike-fbink: statically linked FBInk + touch input ===");

    let fbfd = unsafe { shim_fbink_open() };
    if fbfd < 0 {
        eprintln!("fbink_open failed: {}", fbfd);
        std::process::exit(1);
    }
    println!("fbink_open -> fd {}", fbfd);

    let init_t = Instant::now();
    let rc = unsafe { shim_fbink_init(fbfd, 1) };
    println!("fbink_init -> {} ({:?})", rc, init_t.elapsed());
    if rc < 0 {
        eprintln!("fbink_init failed, aborting");
        std::process::exit(1);
    }

    // (a) device state
    let mut state = ShimState::default();
    unsafe { shim_get_state(&mut state as *mut ShimState) };
    println!("--- fbink_get_state ---");
    println!("device_name:      {}", state.device_name_str());
    println!("device_codename:  {}", state.device_codename_str());
    println!("device_platform:  {}", state.device_platform_str());
    println!("device_id:        {}", state.device_id);
    println!(
        "screen: {}x{} view: {}x{} stride(line_length)={} bpp={}",
        state.screen_width,
        state.screen_height,
        state.view_width,
        state.view_height,
        state.scanline_stride,
        state.bpp
    );
    println!(
        "current_rota={} is_mtk={} is_sunxi={} is_kindle_legacy={} can_wait_for_submission={} pixel_format={}",
        state.current_rota,
        state.is_mtk,
        state.is_sunxi,
        state.is_kindle_legacy,
        state.can_wait_for_submission,
        state.pixel_format
    );
    println!(
        "touch_swap_axes={} touch_mirror_x={} touch_mirror_y={}",
        state.touch_swap_axes, state.touch_mirror_x, state.touch_mirror_y
    );

    // fbink_input_scan: use FBInk's own device classification (as KOReader
    // does) instead of hand-rolling /dev/input enumeration. SCAN_ONLY so we
    // don't hold any fds open here.
    println!("--- fbink_input_scan ---");
    let match_mask = INPUT_TOUCHSCREEN
        | INPUT_SCALED_TABLET
        | INPUT_PAGINATION_BUTTONS
        | INPUT_HOME_BUTTON
        | INPUT_DPAD
        | INPUT_KINDLE_FRAME_TAP;
    let mut devs = [ShimInputDevice::default(); 16];
    let n = unsafe { shim_input_scan(match_mask, devs.as_mut_ptr(), devs.len()) };
    println!(
        "fbink_input_scan(match_mask=0x{:08x}) -> {} device(s) total",
        match_mask, n
    );
    for d in devs.iter().take(n.min(devs.len())) {
        println!(
            "  path={:<24} name={:<32} type=0x{:08x} matched={} fd={}",
            d.path_str(),
            d.name_str(),
            d.type_,
            d.matched,
            d.fd
        );
    }

    // (b) full-screen GC16 flash of a gray ramp + black blocks, via the
    // mmap'd framebuffer pointer (no fbink_print_raw_data -- that path
    // pulls in the IMAGE/stb codepath, which we deliberately excluded from
    // this MINIMAL build).
    let mut fb_size: usize = 0;
    let fb_ptr = unsafe { shim_get_fb_pointer(fbfd, &mut fb_size as *mut usize) };
    if fb_ptr.is_null() {
        eprintln!("fbink_get_fb_pointer failed");
        std::process::exit(1);
    }
    println!("fbink_get_fb_pointer -> {:p} size={}", fb_ptr, fb_size);

    let stride = state.scanline_stride as usize;
    let build_t = Instant::now();
    let mut page = vec![0u8; SCREEN_W * SCREEN_H];
    for y in 0..SCREEN_H {
        for x in 0..SCREEN_W {
            // Gray ramp left->right.
            let mut v = ((x * 255) / SCREEN_W) as u8;
            // A few black text-like blocks scattered down the page.
            let block_row = y / 64;
            let in_block_band = y % 64 < 40;
            if in_block_band && (block_row % 2 == 0) {
                let col = x / 20;
                if col % 3 != 2 {
                    v = 0;
                }
            }
            page[y * SCREEN_W + x] = v;
        }
    }
    println!("full-page buffer build: {:?}", build_t.elapsed());

    let write_t = Instant::now();
    unsafe {
        for y in 0..SCREEN_H {
            let src = &page[y * SCREEN_W..y * SCREEN_W + SCREEN_W];
            let dst = fb_ptr.add(y * stride);
            std::ptr::copy_nonoverlapping(src.as_ptr(), dst, SCREEN_W);
        }
    }
    println!("full-page fb memcpy: {:?}", write_t.elapsed());

    let submit_t = Instant::now();
    let rc = unsafe {
        shim_refresh(
            fbfd,
            0,
            0,
            SCREEN_W as u32,
            SCREEN_H as u32,
            WFM_GC16,
            1, // is_flashing
            0,
        )
    };
    let submit_elapsed = submit_t.elapsed();
    println!(
        "full-page GC16 flash refresh submit -> {} ({:?})",
        rc, submit_elapsed
    );

    let wait_t = Instant::now();
    let rc = unsafe { shim_wait_for_complete(fbfd, 0) };
    println!(
        "wait_for_complete (full GC16) -> {} ({:?})",
        rc,
        wait_t.elapsed()
    );

    // (c) 10 partial updates of a 400x200 region, for REAGL, then GL16, then DU, then A2.
    let region = (418u32, 724u32, 400u32, 200u32); // left, top, width, height centered-ish
    for (name, mode) in [
        ("REAGL", WFM_REAGL),
        ("GL16", WFM_GL16),
        ("DU", WFM_DU),
        ("A2", WFM_A2),
    ] {
        println!("--- 10x partial {} updates, region {:?} ---", name, region);
        for i in 0..10u8 {
            // Toggle the region content a bit each time so there's something to refresh.
            let shade: u8 = if i % 2 == 0 { 40 } else { 220 };
            unsafe {
                for ry in 0..region.3 as usize {
                    let dst = fb_ptr.add((region.1 as usize + ry) * stride + region.0 as usize);
                    std::ptr::write_bytes(dst, shade, region.2 as usize);
                }
            }
            let t = Instant::now();
            let rc = unsafe {
                shim_refresh(
                    fbfd,
                    region.1,
                    region.0,
                    region.2,
                    region.3,
                    mode,
                    0, // partial, not flashing
                    0,
                )
            };
            let submit_elapsed = t.elapsed();
            let wt = Instant::now();
            let wrc = unsafe { shim_wait_for_complete(fbfd, 0) };
            println!(
                "  [{}] submit rc={} submit_time={:?} wait rc={} wait_time={:?}",
                i,
                rc,
                submit_elapsed,
                wrc,
                wt.elapsed()
            );
        }
    }

    // (d) MTK swipe animation + other MTK-only helpers, if supported.
    println!("--- MTK swipe animation & helpers ---");
    if state.is_mtk != 0 {
        // NOTE: fbink_mtk_toggle_auto_reagl(fbfd, false) ("fast mode") makes
        // large DU/GL16/GC16 PARTIAL updates skip auto-upgrading to REAGL,
        // but the documented caveat is that it also makes
        // fbink_wait_for_any_complete() time out forever while fast mode is
        // on. We explicitly re-enable auto-REAGL (toggle=true) so that
        // fbink_wait_for_any_complete below behaves normally; we do not
        // exercise the fast-mode/timeout combo in this spike.
        let reagl_rc = unsafe { shim_mtk_toggle_auto_reagl(fbfd, 1) };
        println!(
            "fbink_mtk_toggle_auto_reagl(fbfd, true) -> {} (keeping auto-REAGL on; toggling it off is documented to make fbink_wait_for_any_complete hang)",
            reagl_rc
        );
        let wait_any_rc = unsafe { shim_wait_for_any_complete(fbfd) };
        println!("fbink_wait_for_any_complete -> {}", wait_any_rc);

        let swipe_rc = unsafe { shim_mtk_set_swipe_data(1 /* UP */, 12) };
        println!("fbink_mtk_set_swipe_data(UP, 12) -> {}", swipe_rc);
        if swipe_rc == 0 {
            unsafe {
                for ry in 0..region.3 as usize {
                    let dst = fb_ptr.add((region.1 as usize + ry) * stride + region.0 as usize);
                    std::ptr::write_bytes(dst, 10, region.2 as usize);
                }
            }
            let t = Instant::now();
            let rc = unsafe {
                shim_refresh(
                    fbfd,
                    region.1,
                    region.0,
                    region.2,
                    region.3,
                    WFM_GC16,
                    0,
                    1, // is_animated
                )
            };
            println!(
                "animated refresh submit rc={} time={:?}",
                rc,
                t.elapsed()
            );
            let wt = Instant::now();
            let wrc = unsafe { shim_wait_for_complete(fbfd, 0) };
            println!("animated refresh wait rc={} time={:?}", wrc, wt.elapsed());
        } else {
            println!("swipe animation not supported/failed (rc={}), skipping", swipe_rc);
        }
    } else {
        println!("device is not reported as MTK (is_mtk=0), skipping swipe animation test");
    }

    // (e) touch input probe, ~8s, non-destructive.
    println!("--- touch input probe ---");
    input::run_touch_probe("/dev/input/event1", Duration::from_secs(8));

    // Cleanup: restore a blank-ish screen isn't our job here (the wrapper
    // script restores the real framework snapshot); just close fbink's fd.
    let rc = unsafe { shim_fbink_close(fbfd) };
    println!("fbink_close -> {}", rc);

    println!("=== spike-fbink done ===");
}
