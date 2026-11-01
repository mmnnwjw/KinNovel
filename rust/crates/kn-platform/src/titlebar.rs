//! 旧固件的原生标题栏会在封面上继续画时钟。按 KOReader 的方式临时缩小窗口, 退出恢复。
//! 只使用本机 X11 的查询和 ConfigureWindow 请求, 不依赖 Xlib 或外部 wmctrl。

use std::io;

fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }

fn saved_geometry(text: &str) -> io::Result<[i32; 4]> {
    let values: Vec<i32> = text.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|_| invalid("invalid saved title bar geometry"))?;
    let geometry: [i32; 4] = values.try_into().map_err(|_| invalid("invalid saved title bar geometry"))?;
    if !(1..=65535).contains(&geometry[2]) || !(1..=65535).contains(&geometry[3]) {
        return Err(invalid("invalid saved title bar size"));
    }
    Ok(geometry)
}

#[cfg(target_os = "linux")]
pub use linux_impl::{hide, restore};

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::{invalid, saved_geometry};
    use std::io::{self, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn u16_at(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) }
    fn u32_at(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
    fn pad(n: usize) -> usize { (n + 3) & !3 }

    struct X11 { stream: UnixStream, root: u32, sequence: u16 }

    impl X11 {
        fn open() -> io::Result<Self> {
            let mut stream = UnixStream::connect("/tmp/.X11-unix/X0")?;
            stream.set_read_timeout(Some(Duration::from_secs(1)))?;
            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
            // Kindle 的本机 X server 允许 root 直接连接; 拒绝时不改窗口, 启动脚本继续原流程。
            stream.write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])?;
            let mut header = [0; 8];
            stream.read_exact(&mut header)?;
            if header[0] != 1 { return Err(invalid("X11 local connection rejected")); }
            let length = u16_at(&header, 6) as usize * 4;
            if !(32..=65536).contains(&length) { return Err(invalid("invalid X11 setup size")); }
            let mut setup = vec![0; length];
            stream.read_exact(&mut setup)?;
            let screen = 32 + pad(u16_at(&setup, 16) as usize) + setup[21] as usize * 8;
            if setup[20] == 0 || screen + 40 > length { return Err(invalid("X11 has no screen")); }
            Ok(Self { stream, root: u32_at(&setup, screen), sequence: 0 })
        }

        fn send(&mut self, opcode: u8, body: &[u8]) -> io::Result<u16> {
            let size = 4 + pad(body.len());
            let mut request = vec![0; size];
            request[0] = opcode;
            request[2..4].copy_from_slice(&((size / 4) as u16).to_le_bytes());
            request[4..4 + body.len()].copy_from_slice(body);
            self.stream.write_all(&request)?;
            self.sequence = self.sequence.wrapping_add(1);
            Ok(self.sequence)
        }

        fn request(&mut self, opcode: u8, body: &[u8]) -> io::Result<Vec<u8>> {
            let sequence = self.send(opcode, body)?;
            for _ in 0..32 {
                let mut header = [0; 32];
                self.stream.read_exact(&mut header)?;
                if header[0] == 0 { return Err(invalid(&format!("X11 error {} on opcode {}", header[1], header[10]))); }
                if header[0] != 1 { continue; }
                let units = u32_at(&header, 4) as usize;
                if units > 16384 { return Err(invalid("X11 reply too large")); }
                let length = units * 4;
                let mut reply = header.to_vec();
                reply.resize(32 + length, 0);
                self.stream.read_exact(&mut reply[32..])?;
                if u16_at(&header, 2) == sequence { return Ok(reply); }
            }
            Err(invalid("X11 reply missing"))
        }

        fn atom(&mut self, name: &str) -> io::Result<u32> {
            let mut body = vec![0; 4 + name.len()];
            body[..2].copy_from_slice(&(name.len() as u16).to_le_bytes());
            body[4..].copy_from_slice(name.as_bytes());
            Ok(u32_at(&self.request(16, &body)?, 8))
        }

        fn property(&mut self, window: u32, atom: u32) -> io::Result<Vec<u8>> {
            let mut body = Vec::new();
            for value in [window, atom, 0, 0, 256] { body.extend_from_slice(&value.to_le_bytes()); }
            self.request(20, &body)
        }

        fn titlebar(&mut self) -> io::Result<u32> {
            let atom = self.atom("_NET_CLIENT_LIST")?;
            let windows = self.property(self.root, atom)?;
            let count = u32_at(&windows, 16) as usize;
            if windows[1] != 32 || count > 256 || count * 4 > windows.len() - 32 || u32_at(&windows, 12) != 0 {
                return Err(invalid("X11 client list unavailable"));
            }
            for chunk in windows[32..32 + count * 4].chunks_exact(4) {
                let window = u32_at(chunk, 0);
                let name = self.property(window, 39)?; // WM_NAME 是 X11 预定义 atom。
                if name[1] == 8 && String::from_utf8_lossy(&name[32..]).contains(":titleBar_ID:") {
                    return Ok(window);
                }
            }
            Err(io::Error::new(io::ErrorKind::NotFound, "native title bar not found"))
        }

        fn geometry(&mut self, window: u32) -> io::Result<[i32; 4]> {
            let reply = self.request(14, &window.to_le_bytes())?;
            Ok([u16_at(&reply, 12) as i16 as i32, u16_at(&reply, 14) as i16 as i32,
                u16_at(&reply, 16) as i32, u16_at(&reply, 18) as i32])
        }

        fn resize(&mut self, window: u32, geometry: [i32; 4]) -> io::Result<()> {
            let mut body = Vec::new();
            body.extend_from_slice(&window.to_le_bytes());
            body.extend_from_slice(&15u16.to_le_bytes()); // CWX | CWY | CWWidth | CWHeight。
            body.extend_from_slice(&[0; 2]);
            for value in geometry { body.extend_from_slice(&value.to_le_bytes()); }
            self.send(12, &body)?;
            Ok(())
        }

        fn resize_and_wait(&mut self, window: u32, geometry: [i32; 4]) -> io::Result<()> {
            let until = Instant::now() + Duration::from_secs(3);
            loop {
                if self.geometry(window)? == geometry { return Ok(()); }
                self.resize(window, geometry)?;
                std::thread::sleep(Duration::from_millis(50));
                if Instant::now() >= until { return Err(io::Error::new(io::ErrorKind::TimedOut, "WM did not restore title bar geometry")); }
            }
        }
    }

    pub fn hide(state: &Path) -> io::Result<()> {
        // 异常退出留下的记录先还原, 不把已缩小的高度误当原始高度。
        if state.exists() { restore(state)?; }
        let mut x = X11::open()?;
        let window = x.titlebar()?;
        let original = x.geometry(window)?;
        if original[3] <= 1 { return Ok(()); }
        std::fs::write(state, format!("{} {} {} {}\n", original[0], original[1], original[2], original[3]))?;
        x.resize_and_wait(window, [original[0], original[1], original[2], 1])?;
        eprintln!("[titlebar] hidden window={window:#x} original={original:?}");
        Ok(())
    }

    pub fn restore(state: &Path) -> io::Result<()> {
        let original = match std::fs::read_to_string(state) {
            Ok(text) => saved_geometry(&text)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        // Home/pillow 恢复时可能短暂占用 X server; 超时后重连, 不复用半截回复。
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let result = (|| {
                let mut x = X11::open()?;
                let window = x.titlebar()?;
                x.resize_and_wait(window, original)?;
                Ok::<_, io::Error>(window)
            })();
            match result {
                Ok(window) => {
                    std::fs::remove_file(state)?;
                    eprintln!("[titlebar] restored window={window:#x} geometry={original:?}");
                    return Ok(());
                }
                Err(e) if Instant::now() >= until => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::saved_geometry;

    #[test]
    fn restore_geometry_record() {
        assert_eq!(saved_geometry("0 0 758 41\n").unwrap(), [0, 0, 758, 41]);
        assert!(saved_geometry("0 0 758").is_err());
        assert!(saved_geometry("0 0 758 0").is_err());
        assert!(saved_geometry("original").is_err());
    }
}
