//! 見本がまだ無い字を推測するための、手がかりの数字（30×30 の白黒）。
//! ゲームの画面から作ったものではない（同梱してよい）:
//! - `BOLD`: 利用者の手書き（`assets/hand_digits.png`）。X パワー・増減・TOTAL の太い字体に合わせて描いたもの
//! - `GAUGE` / `MENU`: Claude が線で描いた細い縦長の数字（`tools/gen_starter.py`。MENU は横に 1.2 倍）
//!
//! 手元の見本 63 字で、正しい字がいちばん近くなった（2026-10-05）。ただ 2 番目との差は小さいので、
//! 推測にだけ使い、計算で確かめられたものを本物の見本にする（learn.rs）。
//! ビット列は `starter_digits.rs`（`python tools/gen_starter.py` で作る。1 行を下位ビットから左→右）

use std::sync::OnceLock;

use crate::matching::{self, Patch};
use crate::templates::Pool;

include!("starter_digits.rs");

/// 手がかりの字の組
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Set {
    Bold,
    Gauge,
    Menu,
}

fn set_for(pool: Pool) -> Option<Set> {
    match pool {
        Pool::Digit | Pool::DigitSmall | Pool::DigitTotal => Some(Set::Bold),
        Pool::DigitGauge => Some(Set::Gauge),
        Pool::DigitMenu => Some(Set::Menu),
        _ => None,
    }
}

fn patch(rows: &[u32; 30]) -> Patch {
    let px = rows.iter().flat_map(|r| (0..30).map(move |x| ((r >> x) & 1) as u8)).collect();
    Patch { w: 30, h: 30, px }
}

/// 比べる形を前もって作ったもの
struct Prepared {
    /// 太さ違い（1 回削った・そのまま・1 回・2 回広げた）
    thick: Vec<Patch>,
    /// 骨にして 2 回広げた
    skel: Patch,
}

fn erode(p: &Patch) -> Patch {
    let inv = Patch { w: p.w, h: p.h, px: p.px.iter().map(|&v| (v == 0) as u8).collect() };
    let d = matching::dilate(&inv, 1);
    Patch { w: p.w, h: p.h, px: d.px.iter().map(|&v| (v == 0) as u8).collect() }
}

fn skel(p: &Patch) -> Patch {
    matching::dilate(&matching::skeleton(p), 2)
}

fn prepare(p: &Patch) -> Prepared {
    Prepared { thick: vec![erode(p), p.clone(), matching::dilate(p, 1), matching::dilate(p, 2)], skel: skel(p) }
}

fn prepared(set: Set) -> &'static [Prepared] {
    static CELLS: [OnceLock<Vec<Prepared>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let (i, src) = match set {
        Set::Bold => (0, &BOLD),
        Set::Gauge => (1, &GAUGE),
        Set::Menu => (2, &MENU),
    };
    CELLS[i].get_or_init(|| src.iter().map(|r| prepare(&patch(r))).collect())
}

/// 似ている度合い（0〜1）。太い字体は手書きと線の太さが違うので、太さを合わせた重なりと、
/// 骨にした重なりの平均（手元の見本で 37/37。どちらか片方だけでは 34〜36）。細い字体はそのまま重ねる
fn similarity(set: Set, g: &Patch, g_skel: &Patch, t: &Prepared) -> f64 {
    match set {
        Set::Bold => {
            let thick = t.thick.iter().map(|v| matching::glyph_iou(g, v)).fold(0.0, f64::max);
            (thick + matching::glyph_iou(g_skel, &t.skel)) / 2.0
        }
        Set::Gauge | Set::Menu => matching::glyph_iou(g, &t.thick[1]),
    }
}

/// 推測した字
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guess {
    pub c: char,
    pub score: f64,
    /// 2 番目に近い字との差
    pub margin: f64,
}

/// 見本の無い字（大きさをそろえた 30×30）を、手がかりの数字で推測する
pub fn guess(pool: Pool, g: &Patch) -> Option<Guess> {
    let sc = ranking(pool, g)?;
    Some(Guess { c: sc[0].0, score: sc[0].1, margin: sc[0].1 - sc[1].1 })
}

/// 10 個の数字すべての似ている度合い（近い順）
pub fn ranking(pool: Pool, g: &Patch) -> Option<Vec<(char, f64)>> {
    let set = set_for(pool)?;
    if g.w != 30 || g.h != 30 {
        return None;
    }
    let g_skel = if set == Set::Bold { skel(g) } else { g.clone() };
    let mut sc: Vec<(char, f64)> = prepared(set)
        .iter()
        .enumerate()
        .map(|(d, t)| (char::from_digit(d as u32, 10).unwrap(), similarity(set, g, &g_skel, t)))
        .collect();
    sc.sort_by(|a, b| b.1.total_cmp(&a.1));
    Some(sc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_starter_digit_reads_itself() {
        for (pool, src) in [(Pool::Digit, &BOLD), (Pool::DigitGauge, &GAUGE), (Pool::DigitMenu, &MENU)] {
            for (d, rows) in src.iter().enumerate() {
                let g = guess(pool, &patch(rows)).unwrap();
                assert_eq!(g.c, char::from_digit(d as u32, 10).unwrap(), "{pool:?} の {d}");
            }
        }
    }

    /// 登録済みの見本（手元）を、手がかりの数字で読む（`-- --ignored starter_reads_templates --nocapture`）
    #[test]
    #[ignore]
    fn starter_reads_templates() {
        let t = crate::templates::Templates::load(&crate::templates::Templates::default_dir()).unwrap();
        let mut all_ok = true;
        for pool in [Pool::Digit, Pool::DigitSmall, Pool::DigitTotal, Pool::DigitGauge, Pool::DigitMenu] {
            let (mut ok, mut n, mut bad) = (0, 0, Vec::new());
            for tm in t.get(pool).iter().filter(|t| t.label.len() == 1) {
                let g = guess(pool, &tm.patch).unwrap();
                n += 1;
                if g.c.to_string() == tm.label {
                    ok += 1;
                } else {
                    bad.push(format!("{}→{}", tm.label, g.c));
                }
            }
            println!("{pool:?}: {ok}/{n} {}", bad.join(" "));
            all_ok &= ok == n;
        }
        assert!(all_ok);
    }
}

/// 録画の 1 枚（ゲーム画面）の数字の場所ごとに、手がかりの数字の順位を並べる
/// （`SRW_IMGS=a.jpg;b.jpg SRW_PLACES=udemae_value,udemae_total cargo test --release -- --ignored measure_starter_rank --nocapture`）
#[test]
#[ignore]
fn measure_starter_rank() {
    let list = std::env::var("SRW_IMGS").unwrap_or_default();
    let places = std::env::var("SRW_PLACES").unwrap_or_default();
    for p in list.split(';').filter(|p| !p.is_empty()) {
        let img = image::open(p).unwrap().to_rgb8();
        // SRW_WIDTH で照合の幅を変えて試す（既定 1024。0 なら縮めない）
        let w: u32 = std::env::var("SRW_WIDTH").ok().and_then(|v| v.parse().ok()).unwrap_or(1024);
        let work = if w == 0 { img.clone() } else { image::imageops::resize(&img, w, w * 9 / 16, image::imageops::FilterType::Triangle) };
        println!("{}", p.rsplit(['/', '\\']).next().unwrap());
        for id in places.split(',').filter(|s| !s.is_empty()) {
            let place = crate::templates::place(id).unwrap();
            let raw = matching::glyphs(&crate::templates::cut(&work, place));
            println!("  raw: {}", raw.iter().map(|g| match g { matching::Glyph::Dot => ".", matching::Glyph::Minus => "-", _ => "#" }).collect::<String>());
            let gs = crate::templates::cut_glyphs(&work, place);
            println!("  {id}: {} 字", gs.len());
            for g in gs {
                match g {
                    matching::Glyph::Shape(g) => {
                        let r = ranking(place.pool, &g).unwrap();
                        println!("    {}", r.iter().map(|(c, v)| format!("{c}{:.2}", v)).collect::<Vec<_>>().join(" "));
                    }
                    other => println!("    {other:?}"),
                }
            }
        }
    }
}
