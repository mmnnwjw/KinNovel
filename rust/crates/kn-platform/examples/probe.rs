//! Device verification example for kn-platform. Run on the Kindle only
//! (FbinkDisplay/InputReader have no real implementation on other targets).
//!
//! Opens FbinkDisplay, prints DeviceInfo, draws a test pattern into a
//! full-size own buffer (not relying on kn-render drawing fns), presents a
//! full GC16 flash, several partial refreshes (REAGL, DU, A2) and a
//! swipe-animated full refresh in each direction (reporting return codes),
//! then opens InputReader + PowerMonitor and prints events for ~10s before
//! exiting cleanly (ungrab, drop everything).
//!
//! Must be run under a takeover wrapper script (see SPIKE-GUIDE.md /
//! spike-fbink's approach): pause the framework, run this, restore on exit.

#[cfg(target_os = "linux")]
fn main() {
    use kn_platform::{
        DeviceInfo, Display, FbinkDisplay, GestureConfig, InputEvent, InputReader, PowerMonitor,
        RefreshRequest, SwipeDir, Waveform,
    };
    use kn_render::{Bitmap, Rect};
    use std::time::{Duration, Instant};

    println!("=== kn-platform probe: FBInk display + evdev input + lipc power ===");

    let mut display = match FbinkDisplay::open() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("FbinkDisplay::open failed: {e}");
            std::process::exit(1);
        }
    };

    let info: DeviceInfo = display.info().clone();
    println!("--- DeviceInfo ---");
    println!("device_name:      {}", info.device_name);
    println!("device_codename:  {}", info.device_codename);
    println!("platform:         {}", info.platform);
    println!("device_id:        {}", info.device_id);
    println!("size:             {}x{} stride={}", info.width, info.height, info.stride);
    println!(
        "is_mtk={} supports_reagl={} supports_swipe={}",
        info.is_mtk, info.supports_reagl, info.supports_swipe
    );
    println!(
        "touch_swap_axes={} touch_mirror_x={} touch_mirror_y={}",
        info.touch_swap_axes, info.touch_mirror_x, info.touch_mirror_y
    );

    // Build a full-size test pattern buffer ourselves (gray ramp + black
    // bands), independent of kn-render's drawing helpers.
    let w = info.width;
    let h = info.height;
    let mut frame = Bitmap::new(w, h, 0xFF);
    for y in 0..h {
        let row = frame.row_mut(y);
        for x in 0..w as usize {
            let mut v = ((x * 255) / w.max(1) as usize) as u8;
            let block_row = y / 64;
            let in_band = y % 64 < 40;
            if in_band && block_row % 2 == 0 && (x / 20) % 3 != 2 {
                v = 0;
            }
            row[x] = v;
        }
    }

    println!("--- full-page GC16 flash ---");
    let t = Instant::now();
    let rc = display.present(
        &frame,
        &RefreshRequest {
            rect: Rect::new(0, 0, w, h),
            waveform: Waveform::Gc16,
            flash: true,
            swipe: None,
        },
    );
    println!("present(GC16, flash) -> {:?} ({:?})", rc, t.elapsed());
    if let Ok(marker) = rc {
        let t = Instant::now();
        display.wait_complete(marker);
        println!("wait_complete -> {:?}", t.elapsed());
    }

    // Partial refresh region, centered-ish.
    let region = Rect::new(
        (w as i32 / 2).saturating_sub(200),
        (h as i32 / 2).saturating_sub(100),
        400.min(w),
        200.min(h),
    );
    for (name, wfm, flash) in [
        ("REAGL", Waveform::Reagl, false),
        ("DU", Waveform::Du, false),
        ("A2", Waveform::A2, false),
    ] {
        // Toggle the region's content so there's something to refresh.
        for y in region.y..region.bottom() {
            if y < 0 || y as u32 >= h {
                continue;
            }
            let row = frame.row_mut(y as u32);
            let x0 = region.x.max(0) as usize;
            let x1 = (region.right().max(0) as usize).min(row.len());
            for px in row[x0..x1].iter_mut() {
                *px = if name == "A2" { 0 } else { 40 };
            }
        }
        let t = Instant::now();
        let rc = display.present(
            &frame,
            &RefreshRequest {
                rect: region,
                waveform: wfm,
                flash,
                swipe: None,
            },
        );
        println!("--- partial {} refresh ---", name);
        println!("present({}, region={:?}) -> {:?} ({:?})", name, region, rc, t.elapsed());
        if let Ok(marker) = rc {
            display.wait_complete(marker);
        }
    }

    // MTK swipe-animated full refresh, one per direction (only meaningful
    // when supports_swipe, but we still report the attempt either way).
    println!("--- swipe-animated full refresh (supports_swipe={}) ---", info.supports_swipe);
    for dir in [SwipeDir::Left, SwipeDir::Right] {
        let t = Instant::now();
        let rc = display.present(
            &frame,
            &RefreshRequest {
                rect: Rect::new(0, 0, w, h),
                waveform: Waveform::Gc16,
                flash: false,
                swipe: Some(dir),
            },
        );
        println!("present(GC16, swipe={:?}) -> {:?} ({:?})", dir, rc, t.elapsed());
        if let Ok(marker) = rc {
            display.wait_complete(marker);
        }
    }

    // Input + power probe, ~10s, non-destructive (nobody will touch the
    // device, so no gestures are expected -- this just verifies
    // open/grab/ungrab/poll mechanics don't crash or hang).
    println!("--- input + power probe (~10s) ---");
    let mut input = match InputReader::open(&info, GestureConfig::default()) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("InputReader::open failed: {e}");
            None
        }
    };
    if let Some(r) = input.as_mut() {
        println!("input fds: {:?}", r.fds());
        match r.grab() {
            Ok(()) => println!("grab() ok"),
            Err(e) => println!("grab() failed: {e}"),
        }
    }

    let mut power = PowerMonitor::start();
    println!("power fd: {:?}", power.fd());

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events_seen = 0usize;
    while Instant::now() < deadline {
        let mut fds: Vec<libc::pollfd> = Vec::new();
        if let Some(r) = input.as_ref() {
            for fd in r.fds() {
                fds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
            }
        }
        if let Some(fd) = power.fd() {
            fds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
        }
        let timeout_ms = power.next_deadline_ms().unwrap_or(500).min(500) as i32;
        if !fds.is_empty() {
            unsafe {
                libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms.max(1));
            }
        } else {
            std::thread::sleep(Duration::from_millis(timeout_ms.max(1) as u64));
        }

        if let Some(r) = input.as_mut() {
            let mut out = Vec::new();
            if let Err(e) = r.read(&mut out) {
                println!("input read error: {e}");
            }
            for ev in out {
                events_seen += 1;
                match ev {
                    InputEvent::Gesture(g) => println!("gesture: {:?}", g),
                    InputEvent::Key { code, pressed } => {
                        println!("key: {:?} pressed={}", code, pressed)
                    }
                }
            }
        }
        let mut power_events = Vec::new();
        power.poll_events(&mut power_events);
        for ev in power_events {
            events_seen += 1;
            println!("power event: {:?}", ev);
        }
    }
    println!("probe loop done, events_seen={}", events_seen);

    if let Some(r) = input.as_mut() {
        match r.ungrab() {
            Ok(()) => println!("ungrab() ok"),
            Err(e) => println!("ungrab() failed: {e}"),
        }
    }
    drop(input);
    drop(power);
    drop(display);

    println!("=== kn-platform probe done ===");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("kn-platform probe is Kindle/Linux-only (FbinkDisplay/InputReader have no host implementation); build with --target armv7-unknown-linux-musleabihf and run on device.");
}
