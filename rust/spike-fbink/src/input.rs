// Minimal raw evdev reader for the touch panel (/dev/input/event1), using
// plain libc: open, EVIOCGRAB, poll, read, EVIOCGABS. No `evdev` crate --
// this keeps the cross build simple and lets us directly verify open/grab/
// ungrab + non-blocking poll semantics, which is what the spike is after.
use std::ffi::CString;
use std::mem::size_of;
use std::os::unix::io::RawFd;
use std::time::{Duration, Instant};

// NOTE: on armv7 (32-bit) with this old kernel, `struct timeval`'s fields are
// 32-bit `long`, not 64-bit -- using i64 here would silently desync `read()`
// framing against the kernel's actual 16-byte struct input_event.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct InputEvent {
    tv_sec: i32,
    tv_usec: i32,
    type_: u16,
    code: u16,
    value: i32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

const EV_TYPE_E: u64 = 0x45; // 'E'

fn ioc(dir: u64, nr: u64, size: u64) -> libc::c_int {
    ((dir << 30) | (size << 16) | (EV_TYPE_E << 8) | nr) as libc::c_int
}

fn eviocgrab() -> libc::c_int {
    // _IOW('E', 0x90, int)
    ioc(1, 0x90, size_of::<i32>() as u64)
}

fn eviocgabs(abs: u64) -> libc::c_int {
    // _IOR('E', 0x40 + abs, struct input_absinfo)
    ioc(2, 0x40 + abs, size_of::<InputAbsInfo>() as u64)
}

const ABS_NAMES: &[(u64, &str)] = &[
    (0x00, "ABS_X"),
    (0x01, "ABS_Y"),
    (0x18, "ABS_PRESSURE"),
    (0x2f, "ABS_MT_SLOT"),
    (0x30, "ABS_MT_TOUCH_MAJOR"),
    (0x31, "ABS_MT_TOUCH_MINOR"),
    (0x32, "ABS_MT_WIDTH_MAJOR"),
    (0x33, "ABS_MT_WIDTH_MINOR"),
    (0x35, "ABS_MT_POSITION_X"),
    (0x36, "ABS_MT_POSITION_Y"),
    (0x37, "ABS_MT_TOOL_TYPE"),
    (0x39, "ABS_MT_TRACKING_ID"),
    (0x3a, "ABS_MT_PRESSURE"),
];

pub fn run_touch_probe(path: &str, duration: Duration) {
    let c_path = CString::new(path).unwrap();
    let fd: RawFd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
    if fd < 0 {
        println!(
            "touch: open({}) failed: {}",
            path,
            std::io::Error::last_os_error()
        );
        return;
    }
    println!("touch: opened {} as fd {}", path, fd);

    // Report ABS ranges before grabbing.
    println!("touch: ABS ranges:");
    for &(code, name) in ABS_NAMES {
        let mut info = InputAbsInfo::default();
        let rc = unsafe { libc::ioctl(fd, eviocgabs(code), &mut info as *mut _) };
        if rc == 0 && (info.minimum != 0 || info.maximum != 0) {
            println!(
                "  {:<20} min={:<8} max={:<8} fuzz={:<4} flat={:<4} resolution={}",
                name, info.minimum, info.maximum, info.fuzz, info.flat, info.resolution
            );
        }
    }

    // Grab, then ungrab, to verify EVIOCGRAB works (exclusive access).
    let grab_rc = unsafe { libc::ioctl(fd, eviocgrab(), 1i32) };
    println!(
        "touch: EVIOCGRAB(1) rc={} (0=ok) errno={}",
        grab_rc,
        if grab_rc != 0 {
            std::io::Error::last_os_error().to_string()
        } else {
            "-".into()
        }
    );
    let ungrab_rc = unsafe { libc::ioctl(fd, eviocgrab(), 0i32) };
    println!("touch: EVIOCGRAB(0) [release] rc={}", ungrab_rc);

    println!(
        "touch: polling non-blocking for {:?} (no touches expected; just verifying the mechanics)",
        duration
    );
    let start = Instant::now();
    let mut n_events: u64 = 0;
    let mut n_poll_calls: u64 = 0;
    let mut n_timeouts: u64 = 0;
    let mut n_wouldblock: u64 = 0;

    while start.elapsed() < duration {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout_ms = 200;
        let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        n_poll_calls += 1;
        if rc == 0 {
            n_timeouts += 1;
            continue;
        }
        if rc < 0 {
            println!("touch: poll() error: {}", std::io::Error::last_os_error());
            break;
        }
        if pfd.revents & libc::POLLIN != 0 {
            let mut ev = InputEvent::default();
            let n = unsafe {
                libc::read(
                    fd,
                    &mut ev as *mut _ as *mut libc::c_void,
                    size_of::<InputEvent>(),
                )
            };
            if n == size_of::<InputEvent>() as isize {
                n_events += 1;
            } else if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EAGAIN) {
                    n_wouldblock += 1;
                } else {
                    println!("touch: read() error: {}", err);
                    break;
                }
            }
        }
    }

    println!(
        "touch: probe done. poll_calls={} timeouts={} events_read={} spurious_wouldblock={}",
        n_poll_calls, n_timeouts, n_events, n_wouldblock
    );

    unsafe {
        libc::close(fd);
    }
    println!("touch: closed fd {}", fd);
}
