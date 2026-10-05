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
    pub(crate) fn scaled(&self, width: u32) -> (u32, u32, u32, u32) {
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
    binary_at(img, roi, margin, WHITE_MIN)
}

/// 色の条件に合う画素の、列のかたまりを数える。幅（基準 1536 の座標）が `w_min`〜`w_max` のものだけ
/// （進行の○の WIN の判子・残っているイカを色で数える。背景の明かりなどの別の大きさのものは数えない）
pub fn count_color_runs(img: &RgbImage, roi: Roi, pred: impl Fn([u8; 3]) -> bool, w_min: u32, w_max: u32) -> usize {
    let p = cut(img, roi, 0, |c| pred(c) as u8);
    let scale = |v: u32| (v as u64 * img.width() as u64 / BASE_W as u64) as u32;
    let (lo, hi) = (scale(w_min).max(1), scale(w_max).max(1));
    let col = |x: u32| (0..p.h).any(|y| p.px[(y * p.w + x) as usize] != 0);
    let (mut n, mut start) = (0, None);
    for x in 0..=p.w {
        match (x < p.w && col(x), start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                if (lo..=hi).contains(&(x - s)) {
                    n += 1;
                }
                start = None;
            }
            _ => {}
        }
    }
    n
}

/// いちばん明るい色（R・G・B の最大）がしきい値以上なら白。暗いパネルの上の色つきの字（メニューの水色・クリーム色）用
pub fn binary_bright(img: &RgbImage, roi: Roi, margin: u32, min: u8) -> Patch {
    cut(img, roi, margin, |[r, g, b]| (r.max(g).max(b) >= min) as u8)
}

/// 橙の字だけを白にする（白い字の文の中の「180p」など）
pub fn binary_orange(img: &RgbImage, roi: Roi, margin: u32) -> Patch {
    cut(img, roi, margin, |[r, g, b]| (r >= 200 && (90..=190).contains(&g) && b < 90) as u8)
}

/// しきい値を変えて 2 値にする（明るい色の上の白い字など。X パワーの増減は水色のしぶきの上で 140）
pub fn binary_at(img: &RgbImage, roi: Roi, margin: u32, min: u8) -> Patch {
    cut(img, roi, margin, |[r, g, b]| (r.min(g).min(b) >= min) as u8)
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

/// 数字の見本をそろえる大きさ（縦横の比は保ち、高さを合わせて横は真ん中に置く）
const GLYPH_H: u32 = 30;
const GLYPH_W: u32 = 30;
/// 行の高さに対してこれより低いかたまりは、小数点かマイナス（形で決まるので見本は要らない）。
/// TOTAL の前の「=」（数字の半分弱の高さ）もここに入る（横に長いのでマイナス扱い）
const DOT_MAX_H: f64 = 0.5;
/// 低いかたまりの真ん中が、行（高い字の上端〜下端）のこの割合より下なら小数点、上ならマイナス。
/// 小さい字では小数点も横に長くなるので、形ではなく高さの位置で分ける
const DOT_MIN_POS: f64 = 0.7;

/// 数字の 1 文字ぶん
#[derive(Clone, Debug)]
pub enum Glyph {
    Dot,
    Minus,
    /// 大きさをそろえた字（見本と比べる）
    Shape(Patch),
}
/// 白がこれより少ない列のかたまりはごみとして捨てる（行の高さに対する割合）
const NOISE_PX: f64 = 0.15;

/// 2 値の ROI を白い列のかたまりで 1 文字ずつに切る
pub fn glyphs(p: &Patch) -> Vec<Glyph> {
    let col_white = |x: u32| (0..p.h).any(|y| p.px[(y * p.w + x) as usize] != 0);
    let mut runs = Vec::new();
    let mut start = None;
    for x in 0..=p.w {
        let white = x < p.w && col_white(x);
        match (white, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    // 各かたまりの上下を詰める
    let boxes: Vec<(u32, u32, u32, u32, u32)> = runs
        .into_iter()
        .filter_map(|(x0, x1)| {
            let rows: Vec<u32> = (0..p.h)
                .filter(|&y| (x0..x1).any(|x| p.px[(y * p.w + x) as usize] != 0))
                .collect();
            let (y0, y1) = (*rows.first()?, *rows.last()? + 1);
            let n: u32 = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| p.px[(y * p.w + x) as usize] as u32)
                .sum();
            Some((x0, x1, y0, y1, n))
        })
        .collect();
    let line_h = boxes.iter().map(|b| b.3 - b.2).max().unwrap_or(0);
    // 行の上端と下端は、高い字の中央値（p の下に出る部分に引っぱられない）
    let median = |mut v: Vec<u32>| {
        v.sort_unstable();
        v.get(v.len() / 2).copied()
    };
    let tall: Vec<_> = boxes.iter().filter(|b| (b.3 - b.2) as f64 >= DOT_MAX_H * line_h as f64).collect();
    let top = median(tall.iter().map(|b| b.2).collect()).unwrap_or(0) as f64;
    let bottom = median(tall.iter().map(|b| b.3).collect()).unwrap_or(p.h) as f64;
    boxes
        .into_iter()
        .filter(|b| b.4 as f64 >= NOISE_PX * line_h as f64)
        .map(|(x0, x1, y0, y1, _)| {
            let (w, h) = (x1 - x0, y1 - y0);
            if (h as f64) < DOT_MAX_H * line_h as f64 {
                let center = (y0 + y1) as f64 / 2.0;
                return if center >= top + DOT_MIN_POS * (bottom - top) { Glyph::Dot } else { Glyph::Minus };
            }
            // 高さを GLYPH_H に合わせ、横は比を保って真ん中に置く（最近傍）
            let sw = ((w as f64 * GLYPH_H as f64 / h as f64).round() as u32).clamp(1, GLYPH_W);
            let off = (GLYPH_W - sw) / 2;
            let mut px = vec![0u8; (GLYPH_W * GLYPH_H) as usize];
            for gy in 0..GLYPH_H {
                for gx in 0..sw {
                    let sx = x0 + gx * w / sw;
                    let sy = y0 + gy * h / GLYPH_H;
                    px[(gy * GLYPH_W + off + gx) as usize] = p.px[(sy * p.w + sx) as usize];
                }
            }
            Glyph::Shape(Patch { w: GLYPH_W, h: GLYPH_H, px })
        })
        .collect()
}

/// そろえた文字どうしの白の重なり（ずらさない）
pub fn glyph_iou(a: &Patch, b: &Patch) -> f64 {
    let (mut and, mut or) = (0u32, 0u32);
    for (x, y) in a.px.iter().zip(&b.px) {
        and += (x & y) as u32;
        or += (x | y) as u32;
    }
    if or == 0 {
        0.0
    } else {
        and as f64 / or as f64
    }
}

/// 白い所を上下左右に `n` 回広げる
pub fn dilate(p: &Patch, n: u32) -> Patch {
    let mut cur = p.px.clone();
    let (w, h) = (p.w as i64, p.h as i64);
    for _ in 0..n {
        let at = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h && cur[(y * w + x) as usize] != 0;
        let next: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| (at(x, y) || at(x - 1, y) || at(x + 1, y) || at(x, y - 1) || at(x, y + 1)) as u8)
            .collect();
        cur = next;
    }
    Patch { w: p.w, h: p.h, px: cur }
}

/// 細線化（Zhang-Suen）。太さの違う字体どうしを、線の通り道だけで比べるのに使う
pub fn skeleton(p: &Patch) -> Patch {
    let (w, h) = (p.w as i64, p.h as i64);
    let mut b: Vec<u8> = p.px.iter().map(|&v| (v != 0) as u8).collect();
    loop {
        let mut changed = false;
        for step in 0..2 {
            let at = |b: &[u8], x: i64, y: i64| -> u8 {
                if x >= 0 && y >= 0 && x < w && y < h {
                    b[(y * w + x) as usize]
                } else {
                    0
                }
            };
            let mut del = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    if at(&b, x, y) == 0 {
                        continue;
                    }
                    // 上から時計回り
                    let n = [
                        at(&b, x, y - 1),
                        at(&b, x + 1, y - 1),
                        at(&b, x + 1, y),
                        at(&b, x + 1, y + 1),
                        at(&b, x, y + 1),
                        at(&b, x - 1, y + 1),
                        at(&b, x - 1, y),
                        at(&b, x - 1, y - 1),
                    ];
                    let count: u8 = n.iter().sum();
                    let trans = (0..8).filter(|&i| n[i] == 0 && n[(i + 1) % 8] == 1).count();
                    let ok = if step == 0 {
                        n[0] * n[2] * n[4] == 0 && n[2] * n[4] * n[6] == 0
                    } else {
                        n[0] * n[2] * n[6] == 0 && n[0] * n[4] * n[6] == 0
                    };
                    if (2..=6).contains(&count) && trans == 1 && ok {
                        del.push((y * w + x) as usize);
                    }
                }
            }
            for i in &del {
                b[*i] = 0;
            }
            changed |= !del.is_empty();
        }
        if !changed {
            break;
        }
    }
    Patch { w: p.w, h: p.h, px: b }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文字の形を描いた 2 値（`#` が白）
    fn draw(rows: &[&str]) -> Patch {
        let w = rows[0].len() as u32;
        let px = rows.iter().flat_map(|r| r.bytes().map(|b| (b == b'#') as u8)).collect();
        Patch { w, h: rows.len() as u32, px }
    }

    #[test]
    fn dot_and_minus_are_told_by_shape() {
        // 「-1.5」: 横棒・縦棒・点・縦棒
        let p = draw(&[
            "......#....##",
            "......#....#.",
            "#####.#....##",
            "#####.#.....#",
            "......#....##",
            "......#.##...",
            "......#.##...",
        ]);
        let g = glyphs(&p);
        assert_eq!(g.len(), 4);
        assert!(matches!(g[0], Glyph::Minus));
        assert!(matches!(g[1], Glyph::Shape(_)));
        assert!(matches!(g[2], Glyph::Dot));
        assert!(matches!(g[3], Glyph::Shape(_)));
    }
}
