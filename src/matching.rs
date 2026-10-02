//! 見本との照合。ゲーム穴の決まった場所（ROI）を切り出し、見本と重ねて一致度を出す。
//!
//! 比べ方は 2 つ用意して見本で選ぶ（2026-10-03）:
//! - 2 値: 白に近い画素（R・G・B がどれもしきい値以上）だけ残し、白の重なり（IoU）で比べる。
//!   チームの色や背景の違いが消える
//! - グレー: 明るさのまま正規化相互相関（NCC）で比べる。縁のぼけや圧縮に強い
//!
//! 配信の位置は 1〜2 px ずれ得るので、どちらもずらしながら一番よいところを取る。

use image::RgbImage;

/// ゲーム穴の基準の幅（1920×1080 で撮ったときのゲーム穴）。ROI はこの座標で書く
const BASE_W: u32 = 1536;

/// ゲーム穴の中の矩形（基準 1536×864 の座標）
#[derive(Clone, Copy, Debug)]
pub struct Roi {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Roi {
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Roi { x, y, w, h }
    }

    /// 撮ったゲーム穴の大きさに合わせる（1024×576 なら 2/3）
    fn scaled(&self, width: u32) -> (u32, u32, u32, u32) {
        let s = |v: u32| (v as u64 * width as u64 / BASE_W as u64) as u32;
        (s(self.x), s(self.y), s(self.w), s(self.h))
    }
}

/// 切り出した明るさ（グレー）または 2 値。`w`×`h` の行優先
#[derive(Clone, Debug)]
pub struct Patch {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// 2 値にするしきい値（R・G・B の最小値がこれ以上なら白）
pub const WHITE_MIN: u8 = 200;

/// ROI を `margin` px 広げて切り出す（ずらして比べる余白）。`f` で画素を 1 バイトにする
fn cut(img: &RgbImage, roi: Roi, margin: u32, f: impl Fn([u8; 3]) -> u8) -> Patch {
    let (x, y, w, h) = roi.scaled(img.width());
    let x0 = x.saturating_sub(margin);
    let y0 = y.saturating_sub(margin);
    let x1 = (x + w + margin).min(img.width());
    let y1 = (y + h + margin).min(img.height());
    let mut px = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
    for yy in y0..y1 {
        for xx in x0..x1 {
            px.push(f(img.get_pixel(xx, yy).0));
        }
    }
    Patch { w: x1 - x0, h: y1 - y0, px }
}

pub fn binary(img: &RgbImage, roi: Roi, margin: u32) -> Patch {
    cut(img, roi, margin, |[r, g, b]| (r.min(g).min(b) >= WHITE_MIN) as u8)
}

pub fn gray(img: &RgbImage, roi: Roi, margin: u32) -> Patch {
    cut(img, roi, margin, |[r, g, b]| {
        ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8
    })
}

/// 見本 `t` を `s`（見本より大きい）の上でずらし、一番よい一致度を返す
fn best(t: &Patch, s: &Patch, score: impl Fn(&Patch, &Patch, u32, u32) -> f64) -> f64 {
    let mut top = f64::MIN;
    for dy in 0..=s.h.saturating_sub(t.h) {
        for dx in 0..=s.w.saturating_sub(t.w) {
            top = top.max(score(t, s, dx, dy));
        }
    }
    top
}

/// 白の重なり（共通部分 / 和集合）。どちらも白が無ければ 0
pub fn iou(t: &Patch, s: &Patch) -> f64 {
    best(t, s, |t, s, dx, dy| {
        let (mut and, mut or) = (0u32, 0u32);
        for y in 0..t.h {
            let tr = &t.px[(y * t.w) as usize..((y + 1) * t.w) as usize];
            let so = ((y + dy) * s.w + dx) as usize;
            let sr = &s.px[so..so + t.w as usize];
            for (a, b) in tr.iter().zip(sr) {
                and += (a & b) as u32;
                or += (a | b) as u32;
            }
        }
        if or == 0 {
            0.0
        } else {
            and as f64 / or as f64
        }
    })
}

/// 正規化相互相関（-1〜1）。明るさが一様なら 0
pub fn ncc(t: &Patch, s: &Patch) -> f64 {
    let n = (t.w * t.h) as f64;
    let tm = t.px.iter().map(|&v| v as f64).sum::<f64>() / n;
    let tv: Vec<f64> = t.px.iter().map(|&v| v as f64 - tm).collect();
    let tn = tv.iter().map(|v| v * v).sum::<f64>().sqrt();
    best(t, s, |t, s, dx, dy| {
        let (mut sum, mut sq) = (0f64, 0f64);
        for y in 0..t.h {
            let so = ((y + dy) * s.w + dx) as usize;
            for &v in &s.px[so..so + t.w as usize] {
                sum += v as f64;
                sq += v as f64 * v as f64;
            }
        }
        let sm = sum / n;
        let sn = (sq - n * sm * sm).max(0.0).sqrt();
        if tn == 0.0 || sn == 0.0 {
            return 0.0;
        }
        let mut dot = 0f64;
        for y in 0..t.h {
            let to = (y * t.w) as usize;
            let so = ((y + dy) * s.w + dx) as usize;
            for x in 0..t.w as usize {
                dot += tv[to + x] * s.px[so + x] as f64;
            }
        }
        dot / (tn * sn)
    })
}
