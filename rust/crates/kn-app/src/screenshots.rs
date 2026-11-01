//! 截图保存到应用目录 screenshots/, 直接导出当前逻辑画面, 不切页或改阅读进度。
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use kn_render::Bitmap;

use crate::store::Paths;

fn save(paths: &Paths, frame: &Bitmap) -> io::Result<PathBuf> {
    let dir = paths.app_dir.join("screenshots");
    std::fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let path = dir.join(format!("screenshot-{stamp}.png"));
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    let result = (|| {
        let mut output = BufWriter::new(file);
        frame.write_png(&mut output).map_err(io::Error::other)?;
        output.flush()
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

pub fn capture(paths: &Paths, frame: &Bitmap) -> Option<String> {
    Some(match save(paths, frame) {
        Ok(path) => {
            eprintln!("[screenshot] {}", path.display());
            "截图已保存到应用目录 screenshots/".to_string()
        }
        Err(e) => {
            eprintln!("[screenshot] 保存失败: {e}");
            format!("截图保存失败: {e}")
        }
    })
}
