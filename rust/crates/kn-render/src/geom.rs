/// 整数像素坐标。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 轴对齐矩形, 左上角 (x, y), 宽高非负。空矩形 (w == 0 || h == 0) 合法。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }

    /// 半开区间命中: x <= px < right, y <= py < bottom。
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    /// 包含两者的最小矩形; 空矩形视为不存在。
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    /// 四边各向外扩 `d` 像素 (d 可为负表示内缩, 内缩到空时返回空矩形)。
    pub fn inflate(&self, d: i32) -> Rect {
        let w = (self.w as i64 + 2 * d as i64).max(0) as u32;
        let h = (self.h as i64 + 2 * d as i64).max(0) as u32;
        Rect::new(self.x - d, self.y - d, w, h)
    }

    pub fn center(&self) -> Point {
        Point { x: self.x + self.w as i32 / 2, y: self.y + self.h as i32 / 2 }
    }
}
