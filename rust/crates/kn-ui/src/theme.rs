/// 配色 (与 Python `Theme` 一致): 日间白底黑字, 夜间反转。
/// 墨水屏上浅灰对比度低, 避免用 > 200 的灰作为承载信息的颜色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub night: bool,
    pub background: u8,
    pub foreground: u8,
    /// 次要文字
    pub muted: u8,
    /// 浅色底 (顶栏)
    pub light: u8,
    /// 分隔线/描边
    pub mid: u8,
}

impl Theme {
    pub fn new(night: bool) -> Self {
        if night {
            Theme { night, background: 0, foreground: 255, muted: 165, light: 40, mid: 85 }
        } else {
            Theme { night, background: 255, foreground: 0, muted: 105, light: 225, mid: 170 }
        }
    }
}
