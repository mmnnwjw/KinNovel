use crate::geom::Rect;

/// L8 灰度位图, 行主序, `stride >= width`。
///
/// 帧缓冲的 stride 可能大于宽度 (KPW5: 宽 1236, stride 1248), 因此所有操作按行处理,
/// 不得假设 `data.len() == width * height`。
#[derive(Clone, Debug)]
pub struct Bitmap {
    width: u32,
    height: u32,
    stride: usize,
    data: Vec<u8>,
}

impl Bitmap {
    /// 新建并填充为 `fill`; stride 等于宽度。
    pub fn new(width: u32, height: u32, fill: u8) -> Self {
        Self::with_stride(width, height, width as usize, fill)
    }

    /// 指定 stride (>= width) 新建, 用于与帧缓冲布局一致的后备缓冲。
    pub fn with_stride(width: u32, height: u32, stride: usize, fill: u8) -> Self {
        let stride = stride.max(width as usize);
        let len = stride * height as usize;
        Bitmap {
            width,
            height,
            stride,
            data: vec![fill; len],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn stride(&self) -> usize {
        self.stride
    }
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }
    /// 原始数据 (含 stride 填充)。
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
    /// 第 y 行的可见像素 (长度 = width)。
    pub fn row(&self, y: u32) -> &[u8] {
        let start = y as usize * self.stride;
        &self.data[start..start + self.width as usize]
    }
    pub fn row_mut(&mut self, y: u32) -> &mut [u8] {
        let start = y as usize * self.stride;
        let w = self.width as usize;
        &mut self.data[start..start + w]
    }

    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.row(y)[x as usize]
    }

    /// 填充矩形 (裁剪到边界)。整行填充必须走 `slice::fill`, 不能逐像素。
    pub fn fill_rect(&mut self, r: Rect, value: u8) {
        let Some(r) = r.intersect(&self.bounds()) else {
            return;
        };
        let x0 = r.x as usize;
        let x1 = r.right() as usize;
        for y in r.y..r.bottom() {
            self.row_mut(y as u32)[x0..x1].fill(value);
        }
    }

    /// 矩形描边, 线宽 `width` 向内。
    pub fn stroke_rect(&mut self, r: Rect, width: u32, value: u8) {
        if r.is_empty() || width == 0 {
            return;
        }
        let lw = width as i32;
        // top
        self.fill_rect(Rect::new(r.x, r.y, r.w, lw.min(r.h as i32).max(0) as u32), value);
        // bottom
        let bh = lw.min(r.h as i32).max(0) as u32;
        self.fill_rect(Rect::new(r.x, r.bottom() - bh as i32, r.w, bh), value);
        // left
        let lww = lw.min(r.w as i32).max(0) as u32;
        self.fill_rect(Rect::new(r.x, r.y, lww, r.h), value);
        // right
        self.fill_rect(Rect::new(r.right() - lww as i32, r.y, lww, r.h), value);
    }

    /// 圆角矩形: 先用 `fill` 填充 (None 表示不填充), 再用 `outline` 描边 `line_width` 像素 (None 表示不描边)。
    ///
    /// 只有四个 radius x radius 角落需要抗锯齿 (按到圆心距离解析计算覆盖率); 矩形内部与四条
    /// 直边都是精确矩形, 用 `fill_rect`/`slice::fill` 整行处理, 不逐像素采样 —— 这样耗时只与
    /// `radius` 相关, 与矩形整体面积无关 (大按钮/列表行每帧重绘时很关键)。
    pub fn rounded_rect(
        &mut self,
        r: Rect,
        radius: u32,
        fill: Option<u8>,
        outline: Option<u8>,
        line_width: u32,
    ) {
        if r.is_empty() {
            return;
        }
        let radius = radius.min(r.w / 2).min(r.h / 2);
        let lw = if outline.is_some() { line_width } else { 0 };

        if radius == 0 {
            // Degenerates to a plain rect: no antialiasing needed at all.
            if let Some(fv) = fill {
                self.fill_rect(r, fv);
            }
            if lw > 0 {
                if let Some(ov) = outline {
                    self.stroke_rect(r, lw, ov);
                }
            }
            return;
        }

        let radius_i = radius as i32;
        let rf = radius as f32;
        let lwf = lw as f32;
        let inner_r = (rf - lwf).max(0.0);

        // ---- exact (non-antialiased) parts, drawn with row-fill / fill_rect ----
        if let Some(fv) = fill {
            // Middle horizontal band (full width), between the top and bottom corner rows.
            self.fill_rect(
                Rect::new(r.x, r.y + radius_i, r.w, r.h.saturating_sub(2 * radius)),
                fv,
            );
            // Top/bottom bands, excluding the corner columns.
            let mid_w = r.w.saturating_sub(2 * radius);
            if mid_w > 0 {
                self.fill_rect(Rect::new(r.x + radius_i, r.y, mid_w, radius), fv);
                self.fill_rect(
                    Rect::new(r.x + radius_i, r.bottom() - radius_i, mid_w, radius),
                    fv,
                );
            }
        }
        if lw > 0 {
            if let Some(ov) = outline {
                let mid_w = r.w.saturating_sub(2 * radius);
                if mid_w > 0 {
                    // Top/bottom straight edges, excluding corners.
                    self.fill_rect(Rect::new(r.x + radius_i, r.y, mid_w, lw), ov);
                    self.fill_rect(
                        Rect::new(r.x + radius_i, r.bottom() - lw as i32, mid_w, lw),
                        ov,
                    );
                }
                let mid_h = r.h.saturating_sub(2 * radius);
                if mid_h > 0 {
                    // Left/right straight edges, excluding corners.
                    self.fill_rect(Rect::new(r.x, r.y + radius_i, lw, mid_h), ov);
                    self.fill_rect(
                        Rect::new(r.right() - lw as i32, r.y + radius_i, lw, mid_h),
                        ov,
                    );
                }
            }
        }

        // ---- 4 corners: analytic antialiasing, O(radius^2) only ----
        let corners = [
            (r.x, r.y, 1i32, 1i32),                                   // top-left
            (r.right() - radius_i, r.y, -1i32, 1i32),                 // top-right
            (r.x, r.bottom() - radius_i, 1i32, -1i32),                // bottom-left
            (r.right() - radius_i, r.bottom() - radius_i, -1i32, -1i32), // bottom-right
        ];
        for (box_x, box_y, sx, sy) in corners {
            let box_rect = Rect::new(box_x, box_y, radius, radius);
            let Some(clip) = box_rect.intersect(&self.bounds()) else {
                continue;
            };
            // Arc center is the corner box's inner corner (offset by sign towards the interior).
            let cx = if sx > 0 { box_x as f32 + rf } else { box_x as f32 };
            let cy = if sy > 0 { box_y as f32 + rf } else { box_y as f32 };
            for y in clip.y..clip.bottom() {
                let py = y as f32 + 0.5;
                let dy = py - cy;
                let row_base = y as usize * self.stride;
                for x in clip.x..clip.right() {
                    let px = x as f32 + 0.5;
                    let dx = px - cx;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let outer = (rf - dist + 0.5).clamp(0.0, 1.0);
                    if outer <= 0.0 {
                        continue;
                    }
                    let idx = row_base + x as usize;
                    if let Some(fv) = fill {
                        self.data[idx] = blend(self.data[idx], fv, outer);
                    }
                    if lw > 0 {
                        if let Some(ov) = outline {
                            let inner = (inner_r - dist + 0.5).clamp(0.0, 1.0);
                            let band = (outer - inner).clamp(0.0, 1.0);
                            if band > 0.0 {
                                self.data[idx] = blend(self.data[idx], ov, band);
                            }
                        }
                    }
                }
            }
        }
    }

    /// 抗锯齿直线 (带线宽, 圆头), 用于图标绘制。
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, value: u8) {
        let half = (width.max(1.0)) / 2.0;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len2 = dx * dx + dy * dy;
        let bounds = self.bounds();
        let min_x = (x0.min(x1) - half - 1.0).floor().max(0.0) as i32;
        let max_x = (x0.max(x1) + half + 1.0).ceil().min(bounds.right() as f32) as i32;
        let min_y = (y0.min(y1) - half - 1.0).floor().max(0.0) as i32;
        let max_y = (y0.max(y1) + half + 1.0).ceil().min(bounds.bottom() as f32) as i32;
        const SS: i32 = 4;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let mut cover = 0u32;
                for sy in 0..SS {
                    let py = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    for sx in 0..SS {
                        let px = x as f32 + (sx as f32 + 0.5) / SS as f32;
                        let t = if len2 > 0.0 {
                            (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let cx = x0 + dx * t;
                        let cy = y0 + dy * t;
                        let ddx = px - cx;
                        let ddy = py - cy;
                        if ddx * ddx + ddy * ddy <= half * half {
                            cover += 1;
                        }
                    }
                }
                if cover == 0 {
                    continue;
                }
                let coverage = cover as f32 / (SS * SS) as f32;
                if x < 0 || y < 0 {
                    continue;
                }
                let (xu, yu) = (x as u32, y as u32);
                if xu >= self.width || yu >= self.height {
                    continue;
                }
                let idx = yu as usize * self.stride + xu as usize;
                self.data[idx] = blend(self.data[idx], value, coverage);
            }
        }
    }

    /// 反相矩形内像素 (按下反馈用)。
    pub fn invert_rect(&mut self, r: Rect) {
        let Some(r) = r.intersect(&self.bounds()) else {
            return;
        };
        let x0 = r.x as usize;
        let x1 = r.right() as usize;
        for y in r.y..r.bottom() {
            for px in &mut self.row_mut(y as u32)[x0..x1] {
                *px = 255 - *px;
            }
        }
    }

    /// 圆角矩形范围内反相 (按下反馈)。圆角边缘按像素覆盖率部分反相, 与 `rounded_rect` 的抗锯齿边缘对齐;
    /// radius 为 0 时等同 `invert_rect`。
    pub fn invert_rounded(&mut self, r: Rect, radius: u32) {
        let radius = radius.min(r.w / 2).min(r.h / 2);
        if radius == 0 {
            self.invert_rect(r);
            return;
        }
        let Some(clip) = r.intersect(&self.bounds()) else {
            return;
        };
        let rad = radius as f32;
        let (top_c, bot_c) = (r.y as f32 + rad, r.bottom() as f32 - rad);
        let (left_c, right_c) = (r.x as f32 + rad, r.right() as f32 - rad);
        for y in clip.y..clip.bottom() {
            let fy = y as f32 + 0.5;
            let dy = if fy < top_c { top_c - fy } else if fy > bot_c { fy - bot_c } else { 0.0 };
            let row = &mut self.row_mut(y as u32)[clip.x as usize..clip.right() as usize];
            if dy == 0.0 {
                for v in row.iter_mut() {
                    *v = 255 - *v;
                }
                continue;
            }
            for (i, v) in row.iter_mut().enumerate() {
                let fx = (clip.x + i as i32) as f32 + 0.5;
                let dx = if fx < left_c { left_c - fx } else if fx > right_c { fx - right_c } else { 0.0 };
                let cov = if dx == 0.0 { 1.0 } else { (rad - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0) };
                if cov > 0.0 {
                    let x = *v as f32;
                    *v = (x + cov * (255.0 - 2.0 * x)).round() as u8;
                }
            }
        }
    }

    /// 把 `src` 的 `src_rect` 区域复制到本位图 (dx, dy) 处; 两边都裁剪。按行 `copy_from_slice`。
    pub fn blit(&mut self, src: &Bitmap, src_rect: Rect, dx: i32, dy: i32) {
        let Some(src_clip) = src_rect.intersect(&src.bounds()) else {
            return;
        };
        // Destination rect corresponding to src_clip placed at (dx, dy).
        let dst_rect = Rect::new(
            dx + (src_clip.x - src_rect.x),
            dy + (src_clip.y - src_rect.y),
            src_clip.w,
            src_clip.h,
        );
        let Some(dst_clip) = dst_rect.intersect(&self.bounds()) else {
            return;
        };
        // Re-derive the corresponding src offset after dst clipping.
        let off_x = dst_clip.x - dst_rect.x;
        let off_y = dst_clip.y - dst_rect.y;
        let sx0 = src_clip.x + off_x;
        let sy0 = src_clip.y + off_y;
        let w = dst_clip.w as usize;
        for row in 0..dst_clip.h {
            let srow = src.row((sy0 + row as i32) as u32);
            let srow = &srow[sx0 as usize..sx0 as usize + w];
            let drow = self.row_mut((dst_clip.y + row as i32) as u32);
            drow[dst_clip.x as usize..dst_clip.x as usize + w].copy_from_slice(srow);
        }
    }

    /// 按覆盖率蒙版绘制单色: dst = round((dst * (255 - cov) + color * cov) / 255)。
    /// `mask` 为紧凑排列的 `mask_w * mask_h` 覆盖率 (0..=255)。这是文字绘制热路径:
    /// 无分支 u16 运算 (cov=0 保持原值, cov=255 得到 color, 都是精确的), 让 LLVM 用 NEON 8 路向量化。
    pub fn blend_mask(&mut self, mask: &[u8], mask_w: u32, mask_h: u32, dx: i32, dy: i32, color: u8) {
        if mask_w == 0 || mask_h == 0 {
            return;
        }
        let mask_rect = Rect::new(dx, dy, mask_w, mask_h);
        let Some(clip) = mask_rect.intersect(&self.bounds()) else {
            return;
        };
        let off_x = (clip.x - dx) as usize;
        let off_y = (clip.y - dy) as usize;
        let w = clip.w as usize;
        let c = color as u16;
        for row in 0..clip.h as usize {
            let m0 = (off_y + row) * mask_w as usize + off_x;
            let mrow = &mask[m0..m0 + w];
            let drow = self.row_mut((clip.y + row as i32) as u32);
            let drow = &mut drow[clip.x as usize..clip.x as usize + w];
            blend_row(drow, mrow, c);
        }
    }

    /// 与 `other` (同尺寸) 比较, 返回所有不同像素的包围盒; 完全相同返回 None。
    /// 用于刷新调度的差分。热路径是"整行相同"的判断: 按 u32 异或后 OR 归约, 循环内无提前退出,
    /// 便于 NEON 向量化 (不用 `a == b`: 那会调用 musl 的逐字节 memcmp, 设备上慢一倍以上)。
    /// 只有确实不同的行才逐字节找左右边界, 并且只扫描当前包围盒之外的部分。
    pub fn diff_bbox(&self, other: &Bitmap) -> Option<Rect> {
        if self.width != other.width || self.height != other.height {
            return None;
        }
        let w = self.width as usize;
        let mut min_x = w;
        let mut max_x = 0usize;
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        for y in 0..self.height {
            let a = &self.row(y)[..w];
            let b = &other.row(y)[..w];
            if rows_equal(a, b) {
                continue;
            }
            // 左边界: 只需看 min_x 之前; 右边界: 只需看 max_x 之后。
            if let Some(x) = (0..min_x).find(|&x| a[x] != b[x]) {
                min_x = x;
            }
            let from = if max_x == 0 && min_y == u32::MAX { 0 } else { max_x + 1 };
            if let Some(x) = (from..w).rev().find(|&x| a[x] != b[x]) {
                max_x = x;
            }
            if min_y == u32::MAX {
                min_y = y;
            }
            max_y = y;
        }
        if min_y == u32::MAX {
            return None;
        }
        Some(Rect::new(
            min_x as i32,
            min_y as i32,
            (max_x - min_x + 1) as u32,
            max_y - min_y + 1,
        ))
    }

    /// 与 `other` 比较, 按纵向分成若干互不相交的变化区域 (KOReader 式的多脏区):
    /// 相隔不少于 `gap` 行未变化的行就分开; 区域多于 `max_regions` 时反复合并间隔最小的相邻两块。
    /// 每块的左右边界是块内所有变化行的并集。完全相同返回空。
    pub fn diff_regions(&self, other: &Bitmap, gap: u32, max_regions: usize) -> Vec<Rect> {
        if self.width != other.width || self.height != other.height {
            return Vec::new();
        }
        let w = self.width as usize;
        // (min_x, max_x, min_y, max_y)
        let mut bands: Vec<(usize, usize, u32, u32)> = Vec::new();
        for y in 0..self.height {
            let a = &self.row(y)[..w];
            let b = &other.row(y)[..w];
            if rows_equal(a, b) {
                continue;
            }
            match bands.last_mut() {
                Some(band) if y - band.3 <= gap => {
                    // 与 diff_bbox 相同: 只扫描当前左右边界之外的部分
                    if let Some(x) = (0..band.0).find(|&x| a[x] != b[x]) {
                        band.0 = x;
                    }
                    if let Some(x) = (band.1 + 1..w).rev().find(|&x| a[x] != b[x]) {
                        band.1 = x;
                    }
                    band.3 = y;
                }
                _ => {
                    let left = (0..w).find(|&x| a[x] != b[x]).unwrap_or(0);
                    let right = (left..w).rev().find(|&x| a[x] != b[x]).unwrap_or(left);
                    bands.push((left, right, y, y));
                }
            }
        }
        let max_regions = max_regions.max(1);
        while bands.len() > max_regions {
            let i = (0..bands.len() - 1).min_by_key(|&i| bands[i + 1].2 - bands[i].3).unwrap_or(0);
            let next = bands.remove(i + 1);
            let band = &mut bands[i];
            band.0 = band.0.min(next.0);
            band.1 = band.1.max(next.1);
            band.3 = next.3;
        }
        bands
            .into_iter()
            .map(|(x0, x1, y0, y1)| Rect::new(x0 as i32, y0 as i32, (x1 - x0 + 1) as u32, y1 - y0 + 1))
            .collect()
    }

    /// 双线性/区域平均缩放到 (w, h), 用于封面与插图。缩小时用区域平均以免锯齿。
    pub fn resize(&self, w: u32, h: u32) -> Bitmap {
        if w == 0 || h == 0 {
            return Bitmap::new(w, h, 0);
        }
        if w == self.width && h == self.height {
            let mut out = Bitmap::new(w, h, 0);
            for y in 0..h {
                out.row_mut(y).copy_from_slice(self.row(y));
            }
            return out;
        }
        if w <= self.width && h <= self.height && self.width > 0 && self.height > 0 {
            self.shrink(w, h)
        } else {
            // 放大 (或一边放大一边缩小): 双线性
            self.bilinear(w, h)
        }
    }

    /// 双线性重采样 (10 位定点, 端点对齐: 输出首尾像素对应源首尾像素)。
    fn bilinear(&self, w: u32, h: u32) -> Bitmap {
        let (sw, sh) = (self.width.max(1) as usize, self.height.max(1) as usize);
        let (dw, dh) = (w as usize, h as usize);
        // 每个输出坐标 -> (源下标, 到下一个源像素的权重 0..1024)
        let axis = |src: usize, dst: usize| -> Vec<(usize, u32)> {
            (0..dst)
                .map(|o| {
                    if dst <= 1 || src <= 1 {
                        return (0, 0);
                    }
                    // o * (src-1) / (dst-1), 定点 1/1024
                    let pos = (o as u64 * (src as u64 - 1) * 1024 + (dst as u64 - 1) / 2) / (dst as u64 - 1);
                    let i = (pos >> 10) as usize;
                    if i >= src - 1 { (src - 1, 0) } else { (i, (pos & 1023) as u32) }
                })
                .collect()
        };
        let xs = axis(sw, dw);
        let ys = axis(sh, dh);
        let mut out = Bitmap::new(w, h, 0);
        for (oy, &(y0, fy)) in ys.iter().enumerate() {
            let y1 = (y0 + 1).min(sh - 1);
            let (r0, r1) = (&self.row(y0 as u32)[..sw], &self.row(y1 as u32)[..sw]);
            let dst = &mut out.row_mut(oy as u32)[..dw];
            for (d, &(x0, fx)) in dst.iter_mut().zip(&xs) {
                let x1 = (x0 + 1).min(sw - 1);
                let top = r0[x0] as u32 * (1024 - fx) + r0[x1] as u32 * fx;
                let bot = r1[x0] as u32 * (1024 - fx) + r1[x1] as u32 * fx;
                *d = ((top * (1024 - fy) + bot * fy + (1 << 19)) >> 20) as u8;
            }
        }
        out
    }


    /// 精确区域平均缩小 (box filter), 全整数, 先横向后纵向。
    /// 源像素 i 在缩放坐标里占 [i*dst, (i+1)*dst), 输出像素 o 占 [o*src, (o+1)*src), 按重叠长度加权。
    /// 设备上比逐像素浮点版快一个数量级 (原版 1350x1920 → 600x853 约 175 ms)。
    /// 区域平均缩小 (box filter): 每个输出像素 = 它覆盖的源区域的面积加权平均。
    /// 先横向 (每行 sw -> dw) 再纵向; 权重是和为 2^16 的定点数, 逐像素只有乘加与移位
    /// (纵向一遍是整行连续的乘加, 可向量化)。
    fn shrink(&self, w: u32, h: u32) -> Bitmap {
        let (sw, sh) = (self.width as usize, self.height as usize);
        let (dw, dh) = (w as usize, h as usize);
        let tx = box_taps(sw, dw);
        let mut tmp = vec![0u8; dw * sh];
        for y in 0..sh {
            let src = &self.row(y as u32)[..sw];
            let dst = &mut tmp[y * dw..(y + 1) * dw];
            match tx.taps {
                1 => shrink_row::<1>(src, dst, &tx),
                2 => shrink_row::<2>(src, dst, &tx),
                3 => shrink_row::<3>(src, dst, &tx),
                4 => shrink_row::<4>(src, dst, &tx),
                _ => shrink_row_any(src, dst, &tx),
            }
        }
        let ty = box_taps(sh, dh);
        let mut out = Bitmap::new(w, h, 0);
        let mut acc = vec![0u32; dw];
        for oy in 0..dh {
            let start = ty.start[oy];
            let weights = &ty.weights[oy * ty.taps..(oy + 1) * ty.taps];
            let row = |k: usize| &tmp[(start + k) * dw..(start + k + 1) * dw];
            let dst = &mut out.row_mut(oy as u32)[..dw];
            // 常见的 2 / 3 个抽头: 一遍算完 (整行连续, 可向量化); 其它按行累加
            match *weights {
                [w0, w1] => {
                    for ((d, &a), &b) in dst.iter_mut().zip(row(0)).zip(row(1)) {
                        *d = ((a as u32 * w0 + b as u32 * w1 + (1 << 15)) >> 16) as u8;
                    }
                }
                [w0, w1, w2] => {
                    for (((d, &a), &b), &c) in dst.iter_mut().zip(row(0)).zip(row(1)).zip(row(2)) {
                        *d = ((a as u32 * w0 + b as u32 * w1 + c as u32 * w2 + (1 << 15)) >> 16) as u8;
                    }
                }
                _ => {
                    acc.iter_mut().for_each(|a| *a = 1 << 15);
                    for (k, &weight) in weights.iter().enumerate() {
                        if weight != 0 {
                            for (a, &v) in acc.iter_mut().zip(row(k)) {
                                *a += v as u32 * weight;
                            }
                        }
                    }
                    for (d, &a) in dst.iter_mut().zip(&acc) {
                        *d = (a >> 16) as u8;
                    }
                }
            }
        }
        out
    }

    /// 等比缩放到能放进 (max_w, max_h) 的最大尺寸 (不放大超过 2 倍)。
    pub fn fit_within(&self, max_w: u32, max_h: u32) -> Bitmap {
        if self.width == 0 || self.height == 0 || max_w == 0 || max_h == 0 {
            return Bitmap::new(0, 0, 0);
        }
        let scale_w = max_w as f64 / self.width as f64;
        let scale_h = max_h as f64 / self.height as f64;
        let scale = scale_w.min(scale_h).min(2.0);
        let w = ((self.width as f64 * scale).round() as u32).max(1);
        let h = ((self.height as f64 * scale).round() as u32).max(1);
        self.resize(w, h)
    }

    /// 把自己按 `scale` (目标像素 / 源像素) 双线性采样到 `out` 的 `dst` 区域:
    /// 目标像素 (x, y) 对应源坐标 (src_x + (x - dst.x) / scale, src_y + (y - dst.y) / scale)。
    /// 落在源图外的目标像素保持不变。用于插图预览的放大/平移 (只渲染可见部分)。
    /// 定点 16.16 运算; 每行先算好源列下标与权重, 内循环只有整数乘加。
    pub fn sample_into(&self, out: &mut Bitmap, dst: Rect, scale: f32, src_x: f32, src_y: f32) {
        let Some(dst) = dst.intersect(&out.bounds()) else { return };
        if self.width == 0 || self.height == 0 || scale <= 0.0 {
            return;
        }
        let inv = 1.0 / scale;
        let (sw, sh) = (self.width as i64, self.height as i64);
        // 列表: (起始目标列, 源列 x0, 权重 0..=256); 源坐标取像素中心
        let mut cols: Vec<(usize, u32, u32)> = Vec::with_capacity(dst.w as usize);
        for dx in 0..dst.w {
            let fx = src_x + (dx as f32 + 0.5) * inv - 0.5;
            if fx < -0.5 || fx > sw as f32 - 0.5 {
                continue;
            }
            let fx = fx.clamp(0.0, (sw - 1) as f32);
            let x0 = fx.floor() as u32;
            let w = ((fx - x0 as f32) * 256.0) as u32;
            cols.push(((dst.x as u32 + dx) as usize, x0, w));
        }
        for dy in 0..dst.h {
            let fy = src_y + (dy as f32 + 0.5) * inv - 0.5;
            if fy < -0.5 || fy > sh as f32 - 0.5 {
                continue;
            }
            let fy = fy.clamp(0.0, (sh - 1) as f32);
            let y0 = fy.floor() as u32;
            let y1 = (y0 + 1).min(sh as u32 - 1);
            let wy = ((fy - y0 as f32) * 256.0) as u32;
            let r0 = self.row(y0);
            let r1 = self.row(y1);
            let last = sw as usize - 1;
            let orow = out.row_mut(dst.y as u32 + dy);
            for &(ox, x0, wx) in &cols {
                let x0 = x0 as usize;
                let x1 = (x0 + 1).min(last);
                let top = r0[x0] as u32 * (256 - wx) + r0[x1] as u32 * wx;
                let bot = r1[x0] as u32 * (256 - wx) + r1[x1] as u32 * wx;
                orow[ox] = ((top * (256 - wy) + bot * wy + (1 << 15)) >> 16) as u8;
            }
        }
    }

    /// 导出为二进制 PGM (P5), 便于在主机上查看/测试快照。
    pub fn to_pgm(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.width as usize * self.height as usize);
        out.extend_from_slice(format!("P5\n{} {}\n255\n", self.width, self.height).as_bytes());
        for y in 0..self.height {
            out.extend_from_slice(self.row(y));
        }
        out
    }
}

/// 单像素覆盖率混合: round((d * (255 - cov) + c * cov) / 255), 精确舍入。
#[inline(always)]
fn blend_px(d: u8, cov: u8, c: u16) -> u8 {
    let cov = cov as u16;
    // t <= 255*255 + 128 = 65153, t + (t >> 8) <= 65407: 全程不溢出 u16。
    let t = d as u16 * (255 - cov) + c * cov + 128;
    ((t + (t >> 8)) >> 8) as u8
}

/// 一行覆盖率混合。简单逐像素循环: 在 `+neon` 下 LLVM 会自动向量化 (u16 8 路)。
/// 实测 (KPW5): 这里受内存带宽限制 —— 手写 NEON 内联汇编与自动向量化同速 (~9 ms / 576 个 48px 字形),
/// 而 target-cpu=cortex-a8 会让成本模型放弃向量化 (慢一倍), 见 rust/.cargo/config.toml。
#[inline]
fn blend_row(drow: &mut [u8], mrow: &[u8], c: u16) {
    for (d, &m) in drow.iter_mut().zip(mrow) {
        *d = blend_px(*d, m, c);
    }
}

/// 一维缩小 (dst <= src) 的定点权重表: 输出像素 o 覆盖从 `start[o]` 起的 `taps` 个源像素,
/// 权重在 `weights[o * taps..]`, 每组之和恰为 2^16 (覆盖不满 taps 个时其余权重为 0)。
struct Taps {
    taps: usize,
    start: Vec<usize>,
    weights: Vec<u32>,
}

fn box_taps(src: usize, dst: usize) -> Taps {
    // 先求精确覆盖: 输出 o 对应源区间 [o*src, (o+1)*src) (单位 1/dst 像素)
    let mut spans: Vec<Vec<(usize, u64)>> = Vec::with_capacity(dst);
    let mut i = 0usize;
    for o in 0..dst {
        let (begin, end) = (o * src, (o + 1) * src);
        let mut cur = begin;
        let mut parts = Vec::with_capacity(src / dst.max(1) + 2);
        while cur < end {
            while (i + 1) * dst <= cur {
                i += 1;
            }
            let seg_end = ((i + 1) * dst).min(end);
            parts.push((i, (seg_end - cur) as u64));
            cur = seg_end;
        }
        spans.push(parts);
    }
    let taps = spans.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let mut start = Vec::with_capacity(dst);
    let mut weights = vec![0u32; dst * taps];
    for (o, parts) in spans.iter().enumerate() {
        // 起点往前挪, 保证 start + taps <= src (挪出来的位置权重为 0)
        let first = parts[0].0;
        let s0 = first.min(src.saturating_sub(taps));
        start.push(s0);
        let total = src as u64;
        let w = &mut weights[o * taps..(o + 1) * taps];
        let mut sum = 0u32;
        let mut largest = first - s0;
        for &(idx, units) in parts {
            let k = idx - s0;
            w[k] = ((units << 16) + total / 2).checked_div(total).unwrap_or(0) as u32;
            sum += w[k];
            if w[k] > w[largest] {
                largest = k;
            }
        }
        // 舍入误差补到最大的权重上, 使和恰为 2^16 (纯色缩小后不变)
        w[largest] = (w[largest] as i64 + (1i64 << 16) - sum as i64) as u32;
    }
    Taps { taps, start, weights }
}

#[inline]
fn shrink_row<const N: usize>(src: &[u8], dst: &mut [u8], t: &Taps) {
    for (o, d) in dst.iter_mut().enumerate() {
        let s = t.start[o];
        let px: &[u8; N] = src[s..s + N].try_into().expect("taps");
        let w: &[u32; N] = t.weights[o * N..(o + 1) * N].try_into().expect("taps");
        let mut acc = 1u32 << 15;
        for k in 0..N {
            acc += px[k] as u32 * w[k];
        }
        *d = (acc >> 16) as u8;
    }
}

fn shrink_row_any(src: &[u8], dst: &mut [u8], t: &Taps) {
    let n = t.taps;
    for (o, d) in dst.iter_mut().enumerate() {
        let s = t.start[o];
        let acc: u32 = src[s..s + n].iter().zip(&t.weights[o * n..(o + 1) * n]).map(|(&p, &w)| p as u32 * w).sum();
        *d = ((acc + (1 << 15)) >> 16) as u8;
    }
}

/// 两行是否完全相同。按 64 字节块做异或 OR 归约, 块内无分支, 块间提前退出。
#[inline]
fn rows_equal(a: &[u8], b: &[u8]) -> bool {
    debug_assert_eq!(a.len(), b.len());
    let mut ca = a.chunks_exact(64);
    let mut cb = b.chunks_exact(64);
    for (x, y) in (&mut ca).zip(&mut cb) {
        let mut acc = 0u32;
        for i in 0..16 {
            let p = u32::from_ne_bytes(x[i * 4..i * 4 + 4].try_into().unwrap());
            let q = u32::from_ne_bytes(y[i * 4..i * 4 + 4].try_into().unwrap());
            acc |= p ^ q;
        }
        if acc != 0 {
            return false;
        }
    }
    ca.remainder().iter().zip(cb.remainder()).all(|(p, q)| p == q)
}

#[inline]
fn blend(dst: u8, color: u8, coverage: f32) -> u8 {
    let d = dst as f32;
    let c = color as f32;
    (d + (c - d) * coverage).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_into_scales_and_offsets() {
        // 2x1 源: 黑, 白; 放大 2 倍到 4x1
        let mut src = Bitmap::new(2, 1, 0);
        src.row_mut(0)[1] = 255;
        let mut out = Bitmap::new(6, 1, 77);
        src.sample_into(&mut out, Rect::new(1, 0, 4, 1), 2.0, 0.0, 0.0);
        let row = out.row(0);
        assert_eq!(row[0], 77, "dst 区域外不动");
        assert_eq!(row[1], 0);
        assert!(row[2] > 0 && row[2] < 128 && row[3] > 128 && row[3] < 255, "{row:?}");
        assert_eq!(row[4], 255);
        assert_eq!(row[5], 77);
        // 平移到源图外: 目标保持不变
        let mut out = Bitmap::new(4, 1, 9);
        src.sample_into(&mut out, Rect::new(0, 0, 4, 1), 1.0, 10.0, 0.0);
        assert_eq!(out.row(0)[..4], [9, 9, 9, 9]);
        // 1:1 原样拷贝
        let mut out = Bitmap::new(2, 1, 9);
        src.sample_into(&mut out, Rect::new(0, 0, 2, 1), 1.0, 0.0, 0.0);
        assert_eq!(out.row(0)[..2], [0, 255]);
    }

    use super::*;

    #[test]
    fn new_fills_and_has_width_stride() {
        let b = Bitmap::new(10, 5, 42);
        assert_eq!(b.width(), 10);
        assert_eq!(b.height(), 5);
        assert_eq!(b.stride(), 10);
        assert!(b.data().iter().all(|&p| p == 42));
    }

    #[test]
    fn with_stride_pads_rows() {
        let b = Bitmap::with_stride(10, 5, 16, 7);
        assert_eq!(b.stride(), 16);
        assert_eq!(b.data().len(), 16 * 5);
        assert_eq!(b.row(0).len(), 10);
    }

    #[test]
    fn fill_rect_clips_to_bounds() {
        let mut b = Bitmap::new(10, 10, 0);
        b.fill_rect(Rect::new(-5, -5, 10, 10), 200);
        // Only the overlap (0,0)-(5,5) should be filled.
        assert_eq!(b.get(0, 0), 200);
        assert_eq!(b.get(4, 4), 200);
        assert_eq!(b.get(5, 5), 0);
        // Fully out-of-bounds rect: no-op, no panic.
        b.fill_rect(Rect::new(100, 100, 5, 5), 9);
    }

    #[test]
    fn stride_buffer_rows_are_isolated() {
        // stride > width: writes to row N must not bleed into row N's padding
        // nor affect row N+1's visible pixels.
        let mut b = Bitmap::with_stride(4, 3, 8, 0);
        b.fill_rect(Rect::new(0, 1, 4, 1), 255);
        assert_eq!(b.row(0), &[0, 0, 0, 0]);
        assert_eq!(b.row(1), &[255, 255, 255, 255]);
        assert_eq!(b.row(2), &[0, 0, 0, 0]);
    }

    #[test]
    fn rounded_rect_bounds_clipping_no_panic() {
        let mut b = Bitmap::new(20, 20, 0);
        // Partially off-canvas on all sides; must clip, not panic.
        b.rounded_rect(Rect::new(-5, -5, 30, 30), 6, Some(200), Some(50), 2);
        // Center should be filled.
        assert_eq!(b.get(10, 10), 200);
        // A fully off-canvas rounded rect is a no-op.
        b.rounded_rect(Rect::new(100, 100, 10, 10), 3, Some(1), None, 0);
    }

    #[test]
    fn rounded_rect_corner_is_antialiased() {
        let mut b = Bitmap::new(40, 40, 0);
        b.rounded_rect(Rect::new(0, 0, 40, 40), 10, Some(255), None, 0);
        // Exact corner pixel (0,0) should be outside the rounded area (not filled).
        assert_eq!(b.get(0, 0), 0);
        // Center is fully inside.
        assert_eq!(b.get(20, 20), 255);
        // Somewhere along the corner arc should show a partially-covered (anti-aliased)
        // pixel, i.e. a value strictly between 0 and 255.
        let found_partial = (0..10)
            .flat_map(|y| (0..10).map(move |x| (x, y)))
            .any(|(x, y)| {
                let v = b.get(x, y);
                v != 0 && v != 255
            });
        assert!(found_partial, "expected an anti-aliased pixel in the corner");
    }

    #[test]
    fn rounded_rect_zero_radius_is_plain_rect() {
        let mut b = Bitmap::new(20, 20, 0);
        b.rounded_rect(Rect::new(2, 2, 10, 10), 0, Some(100), Some(255), 1);
        assert_eq!(b.get(2, 2), 255); // outline corner, exact
        assert_eq!(b.get(6, 6), 100); // interior fill
    }

    #[test]
    fn invert_rect_flips_pixels() {
        let mut b = Bitmap::new(4, 4, 10);
        b.invert_rect(Rect::new(1, 1, 2, 2));
        assert_eq!(b.get(0, 0), 10);
        assert_eq!(b.get(1, 1), 245);
        assert_eq!(b.get(2, 2), 245);
    }

    #[test]
    fn blit_clips_both_sides() {
        let mut src = Bitmap::new(4, 4, 0);
        for y in 0..4 {
            for x in 0..4 {
                src.row_mut(y)[x as usize] = (y * 4 + x) as u8 + 1;
            }
        }
        let mut dst = Bitmap::new(3, 3, 0);
        // Blit the full 4x4 src at (-1,-1): only the inner 3x3 sub-area lands fully
        // inside dst, clipped on both source and destination side.
        dst.blit(&src, src.bounds(), -1, -1);
        // dst(0,0) should equal src(1,1) = 1*4+1+1 = 6
        assert_eq!(dst.get(0, 0), 6);
        assert_eq!(dst.get(2, 2), src.get(3, 3));
    }

    #[test]
    fn blit_fully_out_of_bounds_is_noop() {
        let src = Bitmap::new(4, 4, 255);
        let mut dst = Bitmap::new(4, 4, 0);
        dst.blit(&src, src.bounds(), 100, 100);
        assert!(dst.data().iter().all(|&p| p == 0));
    }

    #[test]
    fn blend_mask_covers_edges_and_full() {
        let mut b = Bitmap::new(4, 4, 0);
        // Row-major 2x2 mask placed at (1,1): (1,1)=0, (2,1)=128, (1,2)=255, (2,2)=0.
        let mask = [0u8, 128, 255, 0];
        b.blend_mask(&mask, 2, 2, 1, 1, 200);
        assert_eq!(b.get(1, 1), 0); // coverage 0: unchanged
        assert_eq!(b.get(1, 2), 200); // coverage 255: full color
        let partial = b.get(2, 1);
        assert!(partial > 0 && partial < 200); // coverage 128: blended
        assert_eq!(b.get(0, 0), 0); // outside mask entirely: untouched
    }

    #[test]
    fn diff_bbox_identical_is_none() {
        let a = Bitmap::new(10, 10, 5);
        let b = Bitmap::new(10, 10, 5);
        assert!(a.diff_bbox(&b).is_none());
    }

    #[test]
    fn invert_rounded_keeps_corners() {
        let mut b = Bitmap::new(40, 30, 255);
        b.invert_rounded(Rect::new(5, 5, 30, 20), 8);
        assert_eq!(b.get(5, 5), 255, "角外不反相");
        assert_eq!(b.get(20, 15), 0, "中间反相");
        assert_eq!(b.get(5, 15), 0, "左边中部反相");
        assert_eq!(b.get(20, 5), 0, "上边中部反相");
        assert_eq!(b.get(4, 15), 255, "矩形外不动");
        let mut c = Bitmap::new(40, 30, 255);
        c.invert_rounded(Rect::new(5, 5, 30, 20), 0);
        assert_eq!(c.get(5, 5), 0);
    }

    #[test]
    fn diff_regions_splits_on_vertical_gaps() {
        let a = Bitmap::new(100, 200, 255);
        let mut b = a.clone();
        b.fill_rect(Rect::new(10, 5, 20, 10), 0);
        b.fill_rect(Rect::new(50, 12, 5, 5), 0);
        b.fill_rect(Rect::new(0, 150, 30, 20), 0);
        assert!(a.diff_regions(&a.clone(), 16, 4).is_empty());
        let r = a.diff_regions(&b, 16, 4);
        assert_eq!(r, vec![Rect::new(10, 5, 45, 12), Rect::new(0, 150, 30, 20)]);
        // gap 不够: 合成一块
        assert_eq!(a.diff_regions(&b, 200, 4), vec![Rect::new(0, 5, 55, 165)]);
        // 上限 1 块: 合并
        assert_eq!(a.diff_regions(&b, 16, 1), vec![Rect::new(0, 5, 55, 165)]);
    }

    #[test]
    fn diff_bbox_single_pixel() {
        let a = Bitmap::new(10, 10, 5);
        let mut b = a.clone();
        b.row_mut(7)[3] = 6;
        let r = a.diff_bbox(&b).expect("should differ");
        assert_eq!(r, Rect::new(3, 7, 1, 1));
    }

    #[test]
    fn diff_bbox_with_stride_padding() {
        // stride > width: padding bytes must not be considered in the diff, and must
        // not cause false positives/negatives.
        let mut a = Bitmap::with_stride(10, 10, 16, 5);
        let mut b = Bitmap::with_stride(10, 10, 16, 5);
        // Make padding differ between the two buffers directly in the raw data.
        a.data_mut()[11] = 99; // byte 11 on row 0 is padding (width=10)
        b.data_mut()[11] = 1;
        assert!(a.diff_bbox(&b).is_none());
        // Now make a real visible-pixel difference.
        b.row_mut(2)[5] = 250;
        let r = a.diff_bbox(&b).expect("should differ");
        assert_eq!(r, Rect::new(5, 2, 1, 1));
    }


    #[test]
    fn shrink_box_filter_is_exact_area_average() {
        // 4x1 → 2x1: (10+30)/2, (50+70)/2
        let mut b = Bitmap::new(4, 1, 0);
        b.row_mut(0)[..4].copy_from_slice(&[10, 30, 50, 70]);
        let r = b.resize(2, 1);
        assert_eq!((r.get(0, 0), r.get(1, 0)), (20, 60));
        // 3x1 → 2x1: 输出0 = (a*2 + b*1)/3, 输出1 = (b*1 + c*2)/3
        let mut b = Bitmap::new(3, 1, 0);
        b.row_mut(0)[..3].copy_from_slice(&[0, 90, 255]);
        let r = b.resize(2, 1);
        assert_eq!((r.get(0, 0), r.get(1, 0)), (30, 200));
        // 纯色缩小后不变
        let b = Bitmap::new(1350, 1920, 77);
        let r = b.resize(600, 853);
        assert!((0..853).all(|y| r.row(y)[..600].iter().all(|&v| v == 77)));
    }

    /// 定点实现与浮点参考 (精确面积平均 / 端点对齐双线性) 相差不超过 1。
    #[test]
    fn fixed_point_resize_matches_float_reference() {
        let (sw, sh) = (143u32, 205u32);
        let mut b = Bitmap::new(sw, sh, 0);
        let mut seed = 12345u32;
        for y in 0..sh {
            for x in 0..sw {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                b.row_mut(y)[x as usize] = (seed >> 16) as u8;
            }
        }
        let px = |x: usize, y: usize| b.row(y as u32)[x] as f64;
        // 缩小: 面积平均
        for &(dw, dh) in &[(115u32, 165u32), (70, 100), (37, 51)] {
            let r = b.resize(dw, dh);
            let (fx, fy) = (sw as f64 / dw as f64, sh as f64 / dh as f64);
            for oy in 0..dh as usize {
                for ox in 0..dw as usize {
                    let (x0, x1, y0, y1) = (ox as f64 * fx, (ox + 1) as f64 * fx, oy as f64 * fy, (oy + 1) as f64 * fy);
                    let mut acc = 0.0;
                    for y in y0.floor() as usize..(y1.ceil() as usize).min(sh as usize) {
                        let wy = (y1.min(y as f64 + 1.0) - y0.max(y as f64)).max(0.0);
                        for x in x0.floor() as usize..(x1.ceil() as usize).min(sw as usize) {
                            let wx = (x1.min(x as f64 + 1.0) - x0.max(x as f64)).max(0.0);
                            acc += px(x, y) * wx * wy;
                        }
                    }
                    let want = acc / (fx * fy);
                    let got = r.row(oy as u32)[ox] as f64;
                    assert!((got - want).abs() <= 1.0, "shrink {dw}x{dh} @({ox},{oy}): {got} vs {want}");
                }
            }
        }
        // 放大: 端点对齐双线性
        let (dw, dh) = (200u32, 300u32);
        let r = b.resize(dw, dh);
        let (sx, sy) = ((sw - 1) as f64 / (dw - 1) as f64, (sh - 1) as f64 / (dh - 1) as f64);
        for oy in 0..dh as usize {
            for ox in 0..dw as usize {
                let (fx, fy) = (ox as f64 * sx, oy as f64 * sy);
                let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
                let (x1, y1) = ((x0 + 1).min(sw as usize - 1), (y0 + 1).min(sh as usize - 1));
                let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
                let top = px(x0, y0) + (px(x1, y0) - px(x0, y0)) * tx;
                let bot = px(x0, y1) + (px(x1, y1) - px(x0, y1)) * tx;
                let want = top + (bot - top) * ty;
                let got = r.row(oy as u32)[ox] as f64;
                assert!((got - want).abs() <= 1.0, "upscale @({ox},{oy}): {got} vs {want}");
            }
        }
    }

    #[test]
    fn resize_dimensions() {
        let b = Bitmap::new(10, 10, 128);
        let up = b.resize(20, 30);
        assert_eq!((up.width(), up.height()), (20, 30));
        let down = b.resize(4, 4);
        assert_eq!((down.width(), down.height()), (4, 4));
        // Downscaling a uniform bitmap should remain uniform.
        assert!(down.data().iter().all(|&p| p == 128));
    }

    #[test]
    fn fit_within_preserves_aspect_and_caps_upscale() {
        let b = Bitmap::new(100, 50, 0);
        let fitted = b.fit_within(40, 40);
        assert_eq!(fitted.width(), 40);
        assert_eq!(fitted.height(), 20);

        // Upscale capped at 2x even if max_w/max_h would allow more.
        let small = Bitmap::new(10, 10, 0);
        let fitted_up = small.fit_within(1000, 1000);
        assert_eq!(fitted_up.width(), 20);
        assert_eq!(fitted_up.height(), 20);
    }

    #[test]
    fn to_pgm_header_and_length() {
        let b = Bitmap::new(3, 2, 9);
        let pgm = b.to_pgm();
        let header = b"P5\n3 2\n255\n";
        assert_eq!(&pgm[..header.len()], header);
        assert_eq!(pgm.len(), header.len() + 6);
        assert!(pgm[header.len()..].iter().all(|&p| p == 9));
    }
}
