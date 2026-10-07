//! バンカラのウデマエのランク（C- 〜 S+50）を、ロビーのメニューの「S+1 284p」の並びから読む。
//!
//! 並びは左にそろい、ランクの英字（一番背が高い）→ 「+」か「-」（あれば）→ S+ の数字（小さく下にそろう）→
//! 間を空けてポイントの数字（背と上端がそろう）→ 最後に p。ランクの長さでポイントの位置が動くので、
//! 右からポイントの数字をまとめ、残った左側をランクとして読む。ポイントの数字はこれまでどおり
//! `menu_udemae_value` の場所で読む（ここでは区切りに使うだけ）。

use std::fmt;

use image::RgbImage;
use serde::{Deserialize, Serialize};

use crate::matching::{self, Glyph, Patch, Roi, WHITE_MIN};

/// 「S+1 284p」の行全体（基準 1536×864。英字の左から p の右まで）
pub const MENU_RANK_LINE: Roi = Roi::new(1322, 184, 192, 38);

/// ランク。`modifier` は -1（「-」）・0・1（「+」）、`num` は S+ の数字
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rank {
    pub letter: char,
    pub modifier: i8,
    pub num: Option<u8>,
}

impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.letter)?;
        match self.modifier {
            1 => write!(f, "+")?,
            -1 => write!(f, "-")?,
            _ => {}
        }
        if let Some(n) = self.num {
            write!(f, "{n}")?;
        }
        Ok(())
    }
}

impl Rank {
    /// 「S+1」「A-」などを読む（出来事の `rank` から）
    pub fn parse(s: &str) -> Option<Rank> {
        let mut it = s.chars();
        let letter = it.next()?;
        let rest: String = it.collect();
        let (modifier, num) = match rest.chars().next() {
            Some('+') => (1, rest[1..].parse().ok()),
            Some('-') => (-1, None),
            None => (0, None),
            _ => return None,
        };
        let r = Rank { letter, modifier, num };
        r.valid().then_some(r)
    }

    /// ゲームにあるランクか（S は「+」「-」なし、S+ は 0〜50、C・B・A は数字なし）
    pub fn valid(&self) -> bool {
        match (self.letter, self.modifier, self.num) {
            ('S', 0, None) => true,
            ('S', 1, Some(n)) => n <= 50,
            ('C' | 'B' | 'A', _, None) => true,
            _ => false,
        }
    }
}

/// 白い列のかたまり（上下は白のある行まで詰めたもの）
#[derive(Clone, Copy, Debug)]
struct Run {
    x0: u32,
    x1: u32,
    top: u32,
    bottom: u32,
}

impl Run {
    fn h(&self) -> u32 {
        self.bottom - self.top + 1
    }
}

fn runs(p: &Patch) -> Vec<Run> {
    let col = |x: u32| (0..p.h).any(|y| p.px[(y * p.w + x) as usize] != 0);
    let mut out = Vec::new();
    let mut start = None;
    for x in 0..=p.w {
        match (x < p.w && col(x), start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                let rows: Vec<u32> = (0..p.h).filter(|&y| (s..x).any(|xx| p.px[(y * p.w + xx) as usize] != 0)).collect();
                // 1〜2 画素の点はぼけた縁や飾りなので捨てる
                if rows.len() >= 3 {
                    out.push(Run { x0: s, x1: x, top: rows[0], bottom: *rows.last().unwrap() });
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// ポイントの数字の始まり（`runs` の番号）。最後のかたまりは p で、その手前から背と上端のそろったものを数字とみる。
/// 数字の手前の平たいかたまりはマイナス（ポイントはマイナスになる）
fn points_start(r: &[Run]) -> Option<usize> {
    let n = r.len();
    if n < 3 {
        return None;
    }
    let d = r[n - 2];
    let tol = (d.h() as f64 * 0.15).max(2.0);
    // 隣の字と背・上端・下端がそろうもの（行は少し傾いているので隣どうしで比べる。
    // 「+」は背が低く下端が上がり、S+ の小さな数字は背が低く上端が下がる）
    let same = |c: &Run, next: &Run| {
        (c.h() as f64 - next.h() as f64).abs() <= tol
            && (c.top as f64 - next.top as f64).abs() <= tol
            && (c.bottom as f64 - next.bottom as f64).abs() <= tol
    };
    // 数字は幅より背が高い。「+」はほぼ正方形で、古い画面では数字と同じくらいの背になる（B+ の見本）
    let digit = |c: &Run| c.x1 - c.x0 < c.h();
    let mut i = n - 2;
    while i > 1 && same(&r[i - 1], &r[i]) && digit(&r[i - 1]) {
        i -= 1;
    }
    if i > 1 {
        let c = r[i - 1];
        let mid = (c.top + c.bottom) / 2;
        if c.h() * 10 < d.h() * 3 && mid > d.top && mid < d.bottom && c.x1 - c.x0 >= c.h() {
            i -= 1;
        }
    }
    Some(i)
}

/// かたまりの白黒の絵（外接の箱）
fn mask(p: &Patch, r: &Run) -> (Vec<bool>, usize, usize) {
    let (w, h) = ((r.x1 - r.x0) as usize, r.h() as usize);
    let px = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| p.px[((r.top + y as u32) * p.w + r.x0 + x as u32) as usize] != 0)
        .collect();
    (px, w, h)
}

/// メニューのランクを読む。`letter` は英字 1 文字の白黒の絵から C・B・A・S を、`digit` は S+ の数字 1 文字を決める
pub fn read_menu(
    work: &RgbImage,
    letter: impl Fn(&[bool], usize, usize) -> Option<char>,
    digit: impl Fn(&Glyph) -> Option<char>,
) -> Option<Rank> {
    let p = matching::binary_at(work, MENU_RANK_LINE, 0, WHITE_MIN);
    let r = runs(&p);
    let s = points_start(&r)?;
    let rank = &r[..s];
    let (head, digits_h) = (rank.first()?, r[r.len() - 2].h());
    // 英字はポイントの数字より背が高い
    if head.h() * 10 < digits_h * 11 {
        return None;
    }
    let (m, w, h) = mask(&p, head);
    let letter = letter(&m, w, h)?;
    // 「-」は横長（幅が背の 1.5 倍以上。太い字では背が英字の 3 割を超えることもある）、「+」はほぼ正方形で英字より低い
    let modifier = match rank.get(1) {
        None => 0,
        Some(c) if (c.x1 - c.x0) * 2 >= c.h() * 3 => -1,
        Some(c) if c.h() * 10 < head.h() * 9 => 1,
        Some(_) => return None,
    };
    let num = if rank.len() > 2 {
        let (x0, x1) = (rank[2].x0, rank[rank.len() - 1].x1);
        let sub = Patch {
            w: x1 - x0,
            h: p.h,
            px: (0..p.h).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| p.px[(y * p.w + x) as usize]).collect(),
        };
        let text: Option<String> = matching::glyphs(&sub).iter().map(&digit).collect();
        Some(text?.parse::<u8>().ok()?)
    } else {
        None
    };
    let rank = Rank { letter, modifier, num };
    rank.valid().then_some(rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_print_and_check() {
        let r = |letter, modifier, num| Rank { letter, modifier, num };
        assert_eq!(r('S', 1, Some(1)).to_string(), "S+1");
        assert_eq!(r('A', -1, None).to_string(), "A-");
        assert_eq!(r('S', 0, None).to_string(), "S");
        assert!(r('S', 1, Some(50)).valid());
        assert!(!r('S', 1, Some(51)).valid());
        assert!(!r('S', 1, None).valid());
        assert!(!r('A', 1, Some(3)).valid());
        assert!(!r('S', -1, None).valid());
    }

    #[test]
    fn points_start_after_the_rank() {
        let run = |x0, x1, top, bottom| Run { x0, x1, top, bottom };
        // 「S+1 284p」: S（背が高い）、+、小さな 1、2 8 4、p（下に出る）
        let r = [run(0, 26, 0, 30), run(29, 46, 8, 26), run(49, 53, 15, 29), run(60, 74, 6, 29), run(76, 90, 6, 29), run(92, 106, 6, 29), run(108, 122, 11, 35)];
        assert_eq!(points_start(&r), Some(3));
        // 「S -40p」: マイナスは数字に含める
        let r = [run(0, 26, 0, 30), run(32, 42, 16, 19), run(45, 59, 6, 29), run(61, 75, 6, 29), run(77, 91, 11, 35)];
        assert_eq!(points_start(&r), Some(1));
        // 「B+ 523p」: 古い画面の「+」は数字と同じくらいの背だが、ほぼ正方形なので数字に含めない
        let r = [run(8, 30, 0, 22), run(33, 55, 0, 18), run(64, 74, 2, 18), run(78, 90, 1, 17), run(93, 105, 0, 16), run(107, 117, 4, 20)];
        assert_eq!(points_start(&r), Some(2));
    }
}

/// 行のかたまりを並べる（`SRW_IMGS=a.png;b.jpg cargo test --release -- --ignored measure_rank_line --nocapture`）
#[test]
#[ignore]
fn measure_rank_line() {
    let list = std::env::var("SRW_IMGS").unwrap_or_default();
    for p in list.split(';').filter(|p| !p.is_empty()) {
        let img = image::open(p).unwrap().to_rgb8();
        let img = image::imageops::resize(&img, 1024, 576, image::imageops::FilterType::Triangle);
        let pt = matching::binary_at(&img, MENU_RANK_LINE, 0, WHITE_MIN);
        let r = runs(&pt);
        println!("{}  {}x{}", p.rsplit(['/', '\\']).next().unwrap(), pt.w, pt.h);
        println!("  runs {:?}", r.iter().map(|r| (r.x0, r.x1, r.top, r.bottom)).collect::<Vec<_>>());
        println!("  points_start {:?}  rank {:?}", points_start(&r), read_menu(&img, crate::shapes::rank_letter, |_| None).map(|r| r.to_string()));
        if let Some(s) = points_start(&r).filter(|s| *s > 2) {
            let (x0, x1) = (r[2].x0, r[s - 1].x1);
            let sub = Patch {
                w: x1 - x0,
                h: pt.h,
                px: (0..pt.h).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| pt.px[(y * pt.w + x) as usize]).collect(),
            };
            let t = crate::templates::Templates::load(&crate::templates::Templates::default_dir()).unwrap();
            for g in matching::glyphs(&sub) {
                let Glyph::Shape(g) = g else { continue };
                let mut sc: Vec<(String, f64)> = t.get(crate::templates::Pool::DigitMenu).iter().map(|tp| (tp.label.clone(), matching::glyph_iou(&g, &tp.patch))).collect();
                sc.sort_by(|a, b| b.1.total_cmp(&a.1));
                println!("  small digit: starter {:?}  templates {:?}", crate::starter::guess(crate::templates::Pool::DigitMenu, &g).map(|s| (s.c, s.score, s.margin)), &sc[..sc.len().min(4)]);
                for y in 0..g.h {
                    println!("      {}", (0..g.w).map(|x| if g.px[(y * g.w + x) as usize] != 0 { '#' } else { '.' }).collect::<String>());
                }
            }
        }
        if let Some(h) = r.first() {
            let (m, w, hh) = mask(&pt, h);
            println!("  letter {:?}  bands {:?}", crate::shapes::rank_letter(&m, w, hh), crate::shapes::rank_letter_bands(&m, w, hh).map(|v| (v * 100.0).round() as i32));
            for y in 0..hh {
                println!("    {}", (0..w).map(|x| if m[y * w + x] { '#' } else { '.' }).collect::<String>());
            }
        }
    }
}
