//! 像素度量：把一张作品图压成几个可比较的数字。
//!
//! 只做"能从图上稳定量出来"的东西；语义层（这个按钮为什么放这）交给视觉模型，
//! 不在这里假装能量出来。

use image::RgbImage;
use image::imageops::FilterType;

pub const MAX_SIDE: u32 = 384;

#[derive(Debug, Clone)]
pub struct Metrics {
    pub width: u32,
    pub height: u32,
    /// 显著边缘像素占比，代理细节密度
    pub edge_density: f32,
    /// 水平/垂直镜像相似度，1 = 完全对称
    pub symmetry: f32,
    /// 最大同色簇占比，代理留白/背景占比
    pub whitespace: f32,
    /// WCAG 对比度 1..21
    pub contrast: f32,
    /// 色板紧凑度 0..1，越高越统一
    pub compactness: f32,
    /// 对齐度 0..1，越高越守网格
    pub alignment: f32,
    /// 主色板：(hex, 面积占比)，按占比降序
    pub palette: Vec<(String, f32)>,
    /// 最深 2% 像素的平均色（正文/线条的候选）
    pub ink: String,
    /// 出现最多的颜色（底色候选）
    pub paper: String,
}

pub fn compute(path: &std::path::Path) -> anyhow::Result<Metrics> {
    // 内容是内容寻址的 blob，没有扩展名，必须按魔数猜格式
    let img = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()?;
    let (ow, oh) = (img.width(), img.height());
    let img = if ow > MAX_SIDE {
        let nh = ((oh as f32) * (MAX_SIDE as f32) / (ow as f32)).max(1.0) as u32;
        img.resize_exact(MAX_SIDE, nh, FilterType::Triangle)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let luma = luma_of(&rgb);
    let (gx, gy) = sobel(&luma, w, h);
    Ok(Metrics {
        width: ow,
        height: oh,
        edge_density: edge_density(&gx, &gy),
        symmetry: symmetry(&gx, &gy, w, h),
        whitespace: whitespace(&rgb),
        contrast: contrast_ratio(&luma),
        compactness: palette_compactness(&rgb),
        alignment: alignment(&gx, &gy, w, h),
        ink: ink_color(&rgb),
        paper: paper_color(&rgb),
        palette: palette(&rgb),
    })
}

fn luma_of(rgb: &RgbImage) -> Vec<f32> {
    rgb.pixels()
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
        .collect()
}

fn idx(x: u32, y: u32, w: u32) -> usize {
    (y as usize) * (w as usize) + x as usize
}

fn sobel(luma: &[f32], w: u32, h: u32) -> (Vec<f32>, Vec<f32>) {
    let mut gx = vec![0.0f32; luma.len()];
    let mut gy = vec![0.0f32; luma.len()];
    if w < 3 || h < 3 {
        return (gx, gy);
    }
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let a = luma[idx(x - 1, y - 1, w)];
            let b = luma[idx(x, y - 1, w)];
            let c = luma[idx(x + 1, y - 1, w)];
            let d = luma[idx(x - 1, y, w)];
            let f = luma[idx(x + 1, y, w)];
            let g = luma[idx(x - 1, y + 1, w)];
            let h2 = luma[idx(x, y + 1, w)];
            let i = luma[idx(x + 1, y + 1, w)];
            let dx = (c + 2.0 * f + i) - (a + 2.0 * d + g);
            let dy = (g + 2.0 * h2 + i) - (a + 2.0 * b + c);
            let k = idx(x, y, w);
            gx[k] = dx / 4.0;
            gy[k] = dy / 4.0;
        }
    }
    (gx, gy)
}

fn edge_density(gx: &[f32], gy: &[f32]) -> f32 {
    if gx.is_empty() {
        return 0.0;
    }
    let mut n = 0usize;
    for (dx, dy) in gx.iter().zip(gy.iter()) {
        if (dx * dx + dy * dy).sqrt() > 0.06 {
            n += 1;
        }
    }
    n as f32 / gx.len() as f32
}

fn symmetry(gx: &[f32], gy: &[f32], w: u32, h: u32) -> f32 {
    if w < 2 || h < 2 {
        return 0.0;
    }
    // 用边缘强度镜像，而不是亮度：整片白底会让亮度镜像假性对称
    let mag: Vec<f32> = gx
        .iter()
        .zip(gy.iter())
        .map(|(a, b)| (a * a + b * b).sqrt().min(1.0))
        .collect();
    let mut dx = 0.0f64;
    let mut n = 0usize;
    for y in 0..h {
        for x in 0..w / 2 {
            dx += (mag[idx(x, y, w)] - mag[idx(w - 1 - x, y, w)]).abs() as f64;
            n += 1;
        }
    }
    let mut dy = 0.0f64;
    let mut m = 0usize;
    for y in 0..h / 2 {
        for x in 0..w {
            dy += (mag[idx(x, y, w)] - mag[idx(x, h - 1 - y, w)]).abs() as f64;
            m += 1;
        }
    }
    let hx = 1.0 - (dx / n as f64 / 0.06).min(1.0);
    let hy = 1.0 - (dy / m as f64 / 0.06).min(1.0);
    (hx.max(hy) as f32).clamp(0.0, 1.0)
}

fn whitespace(rgb: &RgbImage) -> f32 {
    // 低细节瓦片占比：比"主导色占比"更接近"负空间"的含义，
    // 否则一整张高密度报表也会因为白底得到高留白分。
    const TILE: u32 = 16;
    let (w, h) = rgb.dimensions();
    if w < TILE || h < TILE {
        return 0.0;
    }
    let mut empty = 0.0f64;
    let mut total = 0.0f64;
    let mut y = 0;
    while y + TILE <= h {
        let mut x = 0;
        while x + TILE <= w {
            let mut sum = 0.0f64;
            let mut sum2 = 0.0f64;
            for yy in y..y + TILE {
                for xx in x..x + TILE {
                    let p = rgb.get_pixel(xx, yy);
                    let l =
                        (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) as f64;
                    sum += l;
                    sum2 += l * l;
                }
            }
            let n = (TILE * TILE) as f64;
            let mean = sum / n;
            let var = (sum2 / n - mean * mean).max(0.0);
            if var.sqrt() / 255.0 < 0.02 {
                empty += n;
            }
            total += n;
            x += TILE;
        }
        y += TILE;
    }
    if total == 0.0 {
        return 0.0;
    }
    (empty / total) as f32
}

fn contrast_ratio(luma: &[f32]) -> f32 {
    let mut hist = [0u32; 256];
    for l in luma {
        let b = (l * 255.0).clamp(0.0, 255.0) as usize;
        hist[b] += 1;
    }
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return 1.0;
    }
    let pct = |q: f32| -> f32 {
        let target = (q * total as f32) as u32;
        let mut acc = 0u32;
        for (i, c) in hist.iter().enumerate() {
            acc += c;
            if acc >= target {
                return i as f32 / 255.0;
            }
        }
        1.0
    };
    let lin = |v: f32| v.powf(2.2);
    let lo = lin(pct(0.02));
    let hi = lin(pct(0.98));
    ((hi + 0.05) / (lo + 0.05)).clamp(1.0, 21.0)
}

fn palette_compactness(rgb: &RgbImage) -> f32 {
    const K: usize = 6;
    let (w, h) = rgb.dimensions();
    let step = (((w as usize * h as usize) / 20_000).max(1)) as u32;
    let mut pts: Vec<[f32; 3]> = Vec::new();
    for y in (0..h).step_by(step.max(1) as usize) {
        for x in (0..w).step_by(step.max(1) as usize) {
            let p = rgb.get_pixel(x, y);
            pts.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        }
    }
    if pts.len() < K {
        return 0.5;
    }
    let mut centers: Vec<[f32; 3]> = (0..K).map(|k| pts[k * pts.len() / K]).collect();
    let mut assign = vec![0usize; pts.len()];
    for _ in 0..10 {
        for (pi, p) in pts.iter().enumerate() {
            let mut best = 0usize;
            let mut bd = f32::MAX;
            for (ci, c) in centers.iter().enumerate() {
                let d = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
                if d < bd {
                    bd = d;
                    best = ci;
                }
            }
            assign[pi] = best;
        }
        let mut sum = [[0.0f32; 3]; K];
        let mut cnt = [0usize; K];
        for (pi, p) in pts.iter().enumerate() {
            let c = assign[pi];
            for ch in 0..3 {
                sum[c][ch] += p[ch];
            }
            cnt[c] += 1;
        }
        for ci in 0..K {
            if cnt[ci] > 0 {
                for ch in 0..3 {
                    centers[ci][ch] = sum[ci][ch] / cnt[ci] as f32;
                }
            }
        }
    }
    // 簇心相对全局均值的离散度：单色作品接近 0，多色作品显著更大
    let mut g = [0.0f32; 3];
    let mut total = 0f32;
    for p in pts.iter() {
        for ch in 0..3 {
            g[ch] += p[ch];
        }
        total += 1.0;
    }
    for v in g.iter_mut() {
        *v /= total.max(1.0);
    }
    let mut spread = 0.0f32;
    let mut shares = [0f32; K];
    for a in assign.iter() {
        shares[*a] += 1.0;
    }
    for ci in 0..K {
        let share = shares[ci] / total.max(1.0);
        let c = centers[ci];
        let d = ((c[0] - g[0]).powi(2) + (c[1] - g[1]).powi(2) + (c[2] - g[2]).powi(2)).sqrt();
        spread += share * d;
    }
    let norm = spread / 160.0;
    (1.0 - norm.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// 最深 2% 像素的平均色：k-means 按面积取色会漏掉小面积文字色，这里单独补。
fn ink_color(rgb: &RgbImage) -> String {
    let mut px: Vec<([u8; 3], f64)> = rgb
        .pixels()
        .map(|p| {
            let l = 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
            ([p[0], p[1], p[2]], l)
        })
        .collect();
    px.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let n = (px.len() / 50).max(1);
    let mut sum = [0f64; 3];
    for (c, _) in px.iter().take(n) {
        for i in 0..3 {
            sum[i] += c[i] as f64;
        }
    }
    format!(
        "#{:02X}{:02X}{:02X}",
        (sum[0] / n as f64).round() as u8,
        (sum[1] / n as f64).round() as u8,
        (sum[2] / n as f64).round() as u8
    )
}

/// 出现最多的颜色：比"最亮色"稳，深色页面也成立。
fn paper_color(rgb: &RgbImage) -> String {
    let mut bins = [[0u32; 3]; 512];
    let mut counts = [0u32; 512];
    for p in rgb.pixels() {
        let b = ((p[0] >> 5) as usize) << 6 | ((p[1] >> 5) as usize) << 3 | (p[2] >> 5) as usize;
        for i in 0..3 {
            bins[b][i] += p[i] as u32;
        }
        counts[b] += 1;
    }
    let best = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let n = counts[best].max(1);
    format!(
        "#{:02X}{:02X}{:02X}",
        (bins[best][0] / n) as u8,
        (bins[best][1] / n) as u8,
        (bins[best][2] / n) as u8
    )
}

/// 主色板：k-means 的簇心 + 面积占比。颜色是复现里最不能靠形容词的部分，必须给 hex。
fn palette(rgb: &RgbImage) -> Vec<(String, f32)> {
    const K: usize = 5;
    let (w, h) = rgb.dimensions();
    let step = (((w as usize * h as usize) / 20_000).max(1)) as u32;
    let mut pts: Vec<[f32; 3]> = Vec::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let p = rgb.get_pixel(x, y);
            pts.push([p[0] as f32, p[1] as f32, p[2] as f32]);
        }
    }
    if pts.len() < K {
        return Vec::new();
    }
    let mut centers: Vec<[f32; 3]> = (0..K).map(|k| pts[k * pts.len() / K]).collect();
    let mut assign = vec![0usize; pts.len()];
    for _ in 0..12 {
        for (pi, p) in pts.iter().enumerate() {
            let mut best = 0usize;
            let mut bd = f32::MAX;
            for (ci, c) in centers.iter().enumerate() {
                let d = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
                if d < bd {
                    bd = d;
                    best = ci;
                }
            }
            assign[pi] = best;
        }
        let mut sum = [[0.0f32; 3]; K];
        let mut cnt = [0usize; K];
        for (pi, p) in pts.iter().enumerate() {
            let c = assign[pi];
            for ch in 0..3 {
                sum[c][ch] += p[ch];
            }
            cnt[c] += 1;
        }
        for ci in 0..K {
            if cnt[ci] > 0 {
                for ch in 0..3 {
                    centers[ci][ch] = sum[ci][ch] / cnt[ci] as f32;
                }
            }
        }
    }
    let mut shares = [0f32; K];
    for a in assign.iter() {
        shares[*a] += 1.0;
    }
    let total = pts.len() as f32;
    let mut out: Vec<(String, f32)> = centers
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let hex = format!(
                "#{:02X}{:02X}{:02X}",
                c[0].round().clamp(0.0, 255.0) as u8,
                c[1].round().clamp(0.0, 255.0) as u8,
                c[2].round().clamp(0.0, 255.0) as u8
            );
            (hex, shares[i] / total)
        })
        .filter(|(_, s)| *s > 0.02)
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}

fn alignment(gx: &[f32], gy: &[f32], w: u32, h: u32) -> f32 {
    if w < 8 || h < 8 {
        return 0.0;
    }
    let mut strong = vec![0u32; w as usize];
    let mut total = 0u32;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let k = idx(x, y, w);
            let dx = gx[k].abs();
            let dy = gy[k].abs();
            if dx > 0.15 && dx > dy * 1.5 {
                strong[x as usize] += 1;
                total += 1;
            }
        }
    }
    if total == 0 {
        return 0.0;
    }
    let threshold = (h as f32 * 0.10) as u32;
    let mut centers: Vec<u32> = Vec::new();
    let mut run: Option<u32> = None;
    for (x, sc) in strong.iter().enumerate() {
        if *sc >= threshold {
            match run {
                Some(start) if x as u32 - start <= 2 => {}
                Some(start) => {
                    centers.push(start + (x as u32 - start) / 2);
                    run = Some(x as u32);
                }
                None => run = Some(x as u32),
            }
        } else if let Some(start) = run {
            centers.push(start + (x as u32 - start) / 2);
            run = None;
        }
    }
    if let Some(start) = run {
        centers.push(start);
    }
    if centers.len() < 2 {
        return 0.0;
    }
    let mut dist_sum = 0f64;
    let mut n = 0u32;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let k = idx(x, y, w);
            if gx[k].abs() > 0.15 && gx[k].abs() > gy[k].abs() * 1.5 {
                let d = centers
                    .iter()
                    .map(|c| (x as i64 - *c as i64).unsigned_abs() as f32)
                    .fold(f32::MAX, f32::min);
                dist_sum += d as f64;
                n += 1;
            }
        }
    }
    if n == 0 {
        return 0.0;
    }
    let mean = dist_sum / n as f64;
    (1.0 - (mean / (w as f64 / 3.0)).min(1.0)) as f32
}
