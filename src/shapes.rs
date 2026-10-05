//! 見本なしで、画面を形と色で見分ける（ゲームの画面から切り出したものを持たない）。
//!
//! 見本（templates.rs）が無い・足りないときの代わり。決まりごとは手元の見本 89 枚で測った（2026-10-05）:
//! - ウデマエの精算: ゲージの灰色の帯（精算の画面だけ 0.79〜0.96、ほかは 0.22 以下）
//! - X パワー: 真ん中の黒いパネル＋決まった場所の「4 桁.1 桁」の数字（数字の形式は recognize.rs で見る）、
//!   増減の後ろの青緑のしぶき（X パワーの画面だけ 0.47〜0.50）
//! - WIN / LOSE: 左上の白い文字のかたまりの並び（「WIN!」は 4 つ、「LOSE...」は 4 つと低い点 3 つ）
//! - 結果の帯のモード: 白い文字の幅（「Xマッチ」は短く、「バンカラマッチ（…）」は長い）
//! - マッチング: 左上の「マッチメイク中…」のかたまりの並びと、ルール名の色（X は青緑、バンカラはオレンジ）
//! - ロビーのメニュー: 見出しの色（X は青緑、ウデマエはオレンジ）。数字の形式は recognize.rs で見る
//! - ルール紹介: 真ん中の黒いしぶきの白い文字を、Windows のゴシック体で書いた 4 つの語と比べる（フォントは同梱しない）

use std::sync::OnceLock;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use image::RgbImage;

use crate::matching::Roi;
use crate::state::{Mode, Outcome, Rule};

fn ratio(img: &RgbImage, roi: Roi, pred: impl Fn([u8; 3]) -> bool) -> f64 {
    let (x, y, w, h) = roi.scaled(img.width());
    let (mut n, mut all) = (0u32, 0u32);
    for yy in y..(y + h).min(img.height()) {
        for xx in x..(x + w).min(img.width()) {
            n += pred(img.get_pixel(xx, yy).0) as u32;
            all += 1;
        }
    }
    if all == 0 {
        0.0
    } else {
        n as f64 / all as f64
    }
}

fn white([r, g, b]: [u8; 3]) -> bool {
    r.min(g).min(b) >= 200
}

/// 色の無い暗い所（パネル・しぶき）
fn neutral_dark([r, g, b]: [u8; 3]) -> bool {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    mx < 40 && mx - mn < 12
}

/// 色の無い中くらいの灰色（ウデマエのゲージ）
fn mid_gray([r, g, b]: [u8; 3]) -> bool {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    (55..=140).contains(&mx) && mx - mn < 15
}

/// X の青緑
fn teal([r, g, b]: [u8; 3]) -> bool {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    g >= 140 && g - r >= 70 && b >= 90 && g >= b
}

/// バンカラのオレンジ（メニューの見出しのような暗いものも入る）
fn orange([r, g, b]: [u8; 3]) -> bool {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    r >= 50 && r - b >= 50 && g * 10 < r * 6 && g >= b
}

/// ウデマエの精算のゲージ
const GAUGE: Roi = Roi::new(630, 495, 330, 40);
/// X パワーの増減の後ろのしぶき
const SPLASH: Roi = Roi::new(980, 400, 120, 110);
/// 結果の画面の真ん中のパネルの左の縁の内側（文字が来ない所）
const PANEL_EDGE: Roi = Roi::new(415, 300, 40, 300);
/// 左上の文字（WIN! / LOSE... / マッチメイク中...）
const TOP_LEFT: Roi = Roi::new(36, 45, 220, 75);
/// 結果の帯: ルールとステージの黒い帯、その上のモード名の行
const HEADER_BAR: Roi = Roi::new(680, 66, 240, 30);
const HEADER_MODE: Roi = Roi::new(664, 38, 220, 22);
/// マッチングの画面のルール名の行
const MATCHING_RULE: Roi = Roi::new(180, 310, 300, 50);
/// マッチングのメニューの黒いパネル（ルール名の上下の、文字の少ない所）
const MATCHING_PANEL: Roi = Roi::new(120, 280, 500, 240);
/// メニューの見出し（「Xパワー :」「ウデマエ」）
const MENU_X_LABEL: Roi = Roi::new(1288, 156, 108, 42);
const MENU_UDEMAE_LABEL: Roi = Roi::new(1320, 148, 90, 32);
/// ルール紹介の黒いしぶきと、2 行目の語
const INTRO_SPLAT: Roi = Roi::new(600, 300, 350, 230);
const INTRO_WORD: Roi = Roi::new(640, 405, 260, 95);

pub fn udemae_gauge(img: &RgbImage) -> bool {
    ratio(img, GAUGE, mid_gray) >= 0.6
}

pub fn x_splash(img: &RgbImage) -> bool {
    ratio(img, SPLASH, teal) >= 0.3
}

pub fn result_panel(img: &RgbImage) -> bool {
    ratio(img, PANEL_EDGE, neutral_dark) >= 0.9
}

/// 白い文字のかたまりを左から: 背の高いものは `X`、低いもの（点）は `.`
fn blob_pattern(img: &RgbImage, roi: Roi) -> String {
    let (x0, y0, w, h) = roi.scaled(img.width());
    let at = |x: u32, y: u32| x < img.width() && y < img.height() && white(img.get_pixel(x, y).0);
    let mut runs: Vec<(u32, u32, u32)> = Vec::new(); // (幅, 高さ, 上端)
    let mut start = None;
    for x in x0..=x0 + w {
        let any = x < x0 + w && (y0..y0 + h).any(|y| at(x, y));
        match (any, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                let ys: Vec<u32> = (y0..y0 + h).filter(|&y| (s..x).any(|xx| at(xx, y))).collect();
                runs.push((x - s, ys.last().unwrap() - ys[0] + 1, ys[0]));
                start = None;
            }
            _ => {}
        }
    }
    let tall = runs.iter().map(|r| r.1).max().unwrap_or(0);
    // 幅 1px のものはごみ（照合する大きさで）
    runs.iter().filter(|r| r.0 >= 2 || r.1 * 2 >= tall).map(|r| if r.1 * 2 < tall { '.' } else { 'X' }).collect()
}

pub fn outcome(img: &RgbImage) -> Option<Outcome> {
    match blob_pattern(img, TOP_LEFT).as_str() {
        "XXXX" => Some(Outcome::Win),
        "XXXX..." => Some(Outcome::Lose),
        _ => None,
    }
}

/// 結果の帯のモード（X かバンカラか）
pub fn header_mode(img: &RgbImage) -> Option<Mode> {
    // 帯は写真の上の半透明の黒（手元の見本で 0.33〜0.61）
    if ratio(img, HEADER_BAR, neutral_dark) < 0.3 {
        return None;
    }
    let (x0, y0, w, h) = HEADER_MODE.scaled(img.width());
    // 「（チャレンジ）」の小さな字は縮めると細くなるので、白が 1px でもある列を数える
    let cols: Vec<u32> = (x0..x0 + w).filter(|&x| (y0..y0 + h).any(|y| white(img.get_pixel(x, y).0))).collect();
    let (first, last) = (*cols.first()?, *cols.last()?);
    let base = |v: u32| v * 1536 / img.width();
    // 書き出しは帯の左の決まった所（手元の見本で 676〜678）。右端まで白いものは帯ではない
    if !(668..=690).contains(&base(first)) || last + 1 >= x0 + w {
        return None;
    }
    match base(last - first + 1) {
        25..=60 => Some(Mode::X),
        // 「バンカラマッチ（チャレンジ）」「（オープン）」（手元の見本で 76〜176。縮めた大きさで測るので短めに出る）
        70..=200 => Some(Mode::BankaraChallenge),
        _ => None,
    }
}

/// マッチング中の画面と、そのモード
pub fn matching_mode(img: &RgbImage) -> Option<Mode> {
    let p = blob_pattern(img, TOP_LEFT);
    if !(p.starts_with("X.XXXX") && p.ends_with("...")) {
        return None;
    }
    // マッチングのメニューが開いていること（結果の表の上でマッチメイクしているときは、表の色が混じる）
    if ratio(img, MATCHING_PANEL, neutral_dark) < 0.6 {
        return None;
    }
    mode_color(img, MATCHING_RULE, 0.08)
}

/// メニューの見出しの色（数字の形式と合わせて使う）
pub fn menu_x_label(img: &RgbImage) -> bool {
    ratio(img, MENU_X_LABEL, teal) >= 0.05
}

pub fn menu_udemae_label(img: &RgbImage) -> bool {
    ratio(img, MENU_UDEMAE_LABEL, orange) >= 0.15 && ratio(img, MENU_UDEMAE_LABEL, teal) < 0.02
}

fn mode_color(img: &RgbImage, roi: Roi, min: f64) -> Option<Mode> {
    let (t, o) = (ratio(img, roi, teal), ratio(img, roi, orange));
    if t >= min && t > o * 2.0 {
        Some(Mode::X)
    } else if o >= min && o > t * 2.0 {
        Some(Mode::BankaraChallenge)
    } else {
        None
    }
}

// ---- ルール紹介の語 ----

const WORDS: [(Rule, &str); 4] = [(Rule::Area, "エリア"), (Rule::Yagura, "ヤグラ"), (Rule::Hoko, "バトル"), (Rule::Asari, "アサリ")];
/// 比べる大きさ（語を外接の箱いっぱいに伸ばす）
const WW: usize = 96;
const WH: usize = 32;
/// 一番近い語と 2 番目の差（手元の 4 枚で 0.02〜0.16。4 つに 1 つ選ぶだけで、別の画面は白の量と黒いしぶきで先に除く）
const WORD_MARGIN: f64 = 0.015;
const WORD_MIN: f64 = 0.3;

/// 白黒の絵を外接の箱で切り、WW×WH に伸ばして 1 回太らせる
fn fit(px: &[bool], w: usize, h: usize) -> Option<Vec<bool>> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if px[y * w + x] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 == usize::MAX {
        return None;
    }
    let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut out = vec![false; WW * WH];
    for y in 0..WH {
        for x in 0..WW {
            out[y * WW + x] = px[(y0 + y * bh / WH) * w + x0 + x * bw / WW];
        }
    }
    let at = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < WW && (y as usize) < WH && out[y as usize * WW + x as usize];
    Some(
        (0..WH as i64)
            .flat_map(|y| (0..WW as i64).map(move |x| (x, y)))
            .map(|(x, y)| at(x, y) || at(x - 1, y) || at(x + 1, y) || at(x, y - 1) || at(x, y + 1))
            .collect(),
    )
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let (mut and, mut or) = (0, 0);
    for (x, y) in a.iter().zip(b) {
        and += (*x && *y) as u32;
        or += (*x || *y) as u32;
    }
    if or == 0 {
        0.0
    } else {
        and as f64 / or as f64
    }
}

/// Windows のゴシック体で書いた語（フォントごと）。フォントが 1 つも無ければ空
fn words() -> &'static Vec<Vec<Vec<bool>>> {
    static W: OnceLock<Vec<Vec<Vec<bool>>>> = OnceLock::new();
    W.get_or_init(|| {
        let mut out = Vec::new();
        for name in ["meiryob.ttc", "YuGothB.ttc", "BIZ-UDGothicB.ttc"] {
            let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{name}")) else { continue };
            let Ok(font) = ab_glyph::FontVec::try_from_vec_and_index(bytes, 0) else { continue };
            let font = FontArc::new(font);
            let set: Option<Vec<Vec<bool>>> = WORDS.iter().map(|(_, w)| render(&font, w)).collect();
            if let Some(set) = set {
                out.push(set);
            }
        }
        out
    })
}

fn render(font: &FontArc, text: &str) -> Option<Vec<bool>> {
    let scale = PxScale::from(120.0);
    let sf = font.as_scaled(scale);
    let (w, h) = (600usize, 200usize);
    let mut px = vec![false; w * h];
    let mut x = 10.0;
    for c in text.chars() {
        let g = sf.scaled_glyph(c);
        let adv = sf.h_advance(g.id);
        let g = ab_glyph::Glyph { position: ab_glyph::point(x, 20.0 + sf.ascent()), ..g };
        if let Some(o) = font.outline_glyph(g) {
            let bb = o.px_bounds();
            o.draw(|gx, gy, v| {
                let (px_x, px_y) = (bb.min.x as i64 + gx as i64, bb.min.y as i64 + gy as i64);
                if v >= 0.5 && px_x >= 0 && px_y >= 0 && (px_x as usize) < w && (px_y as usize) < h {
                    px[px_y as usize * w + px_x as usize] = true;
                }
            });
        }
        x += adv;
    }
    fit(&px, w, h)
}

/// ルール紹介の画面なら、そのルール
pub fn rule_intro(img: &RgbImage) -> Option<Rule> {
    let wr = ratio(img, INTRO_WORD, white);
    if !(0.18..=0.45).contains(&wr) || ratio(img, INTRO_SPLAT, neutral_dark) < 0.5 {
        return None;
    }
    let (x0, y0, w, h) = INTRO_WORD.scaled(img.width());
    let px: Vec<bool> = (y0..y0 + h)
        .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
        .map(|(x, y)| white(img.get_pixel(x, y).0))
        .collect();
    let g = fit(&px, w as usize, h as usize)?;
    let sets = words();
    if sets.is_empty() {
        return None;
    }
    let mut sc: Vec<(Rule, f64)> = WORDS
        .iter()
        .enumerate()
        .map(|(i, (r, _))| (*r, sets.iter().map(|s| iou(&g, &s[i])).sum::<f64>() / sets.len() as f64))
        .collect();
    sc.sort_by(|a, b| b.1.total_cmp(&a.1));
    (sc[0].1 >= WORD_MIN && sc[0].1 - sc[1].1 >= WORD_MARGIN).then_some(sc[0].0)
}

/// 手元の見本（samples/snaps）で、全部の決まりごとの当たり外れを見る
/// （`cargo test --release -- --ignored shapes_on_snaps --nocapture`）
#[cfg(test)]
mod with_samples {
    use super::*;

    #[test]
    #[ignore]
    fn shapes_on_snaps() {
        let dir = crate::samples_dir().join("snaps");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "png") && !p.to_string_lossy().ends_with("_ほこ.png"))
            .collect();
        files.sort();
        for p in files {
            let img = crate::templates::to_work(&image::open(&p).unwrap().to_rgb8());
            let name = p.file_stem().unwrap().to_string_lossy().replace("20261003-", "");
            let mut found = Vec::new();
            if udemae_gauge(&img) {
                found.push("ゲージ".to_string());
            }
            if x_splash(&img) {
                found.push("しぶき".into());
            }
            if result_panel(&img) {
                found.push("パネル".into());
            }
            if let Some(o) = outcome(&img) {
                found.push(format!("{o:?}"));
            }
            if let Some(m) = header_mode(&img) {
                found.push(format!("帯{m:?}"));
            }
            if let Some(m) = matching_mode(&img) {
                found.push(format!("マッチング{m:?}"));
            }
            if menu_x_label(&img) {
                found.push("メニューX".into());
            }
            if menu_udemae_label(&img) {
                found.push("メニューウデマエ".into());
            }
            if let Some(r) = rule_intro(&img) {
                found.push(format!("紹介{r:?}"));
            }
            println!("{:<40} {}", name.chars().take(26).collect::<String>(), found.join(" "));
        }
    }
}

/// 1 フレームの照合にかかる時間（見本なし・手元の見本あり）。`-- --ignored time_per_frame --nocapture`
#[cfg(test)]
mod timing {
    #[test]
    #[ignore]
    fn time_per_frame() {
        use crate::recognize::Recognizer;
        use crate::templates::Templates;
        let dir = crate::samples_dir().join("snaps");
        let empty = std::env::temp_dir().join(format!("srw-time-{}", std::process::id()));
        for (what, t) in [("見本なし", Templates::load(&empty).unwrap()), ("見本あり", Templates::load(&Templates::default_dir()).unwrap())] {
            let rec = Recognizer::new(t);
            for key in ["134014", "033345", "040440", "031924"] {
                let p = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).find(|p| p.to_string_lossy().contains(key)).unwrap();
                let img = crate::templates::to_work(&image::open(p).unwrap().to_rgb8());
                let t0 = std::time::Instant::now();
                for _ in 0..20 {
                    rec.recognize(&img);
                }
                println!("{what} {key}: {:.1}ms", t0.elapsed().as_secs_f64() * 1000.0 / 20.0);
            }
        }
        let _ = std::fs::remove_dir_all(&empty);
    }
}
