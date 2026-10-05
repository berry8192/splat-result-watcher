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
/// 結果の帯のモード名の行（字と、その後ろの黒い箱の長さを測る）
const MODE_ROW: Roi = Roi::new(650, 40, 300, 18);
/// 結果の表（個人リザルト・スコアボード）の右の黒いパネル
const SCOREBOARD: Roi = Roi::new(1150, 250, 200, 300);
/// マッチングの画面のルール名の行
const MATCHING_RULE: Roi = Roi::new(180, 310, 300, 50);
/// マッチングのメニューの黒いパネル（ルール名の上下の、文字の少ない所）
const MATCHING_PANEL: Roi = Roi::new(120, 280, 500, 240);
/// バンカラのマッチングの値の右の、チャレンジの○
const MATCHING_CIRCLES: Roi = Roi::new(360, 415, 260, 50);
/// メニューの値の後ろの黒い箱（本物のメニューは 0.54〜0.91、試合中にインクの色を見出しと見たときは 0.16 以下）
const MENU_X_BOX: Roi = Roi::new(1390, 156, 120, 42);
const MENU_UDEMAE_BOX: Roi = Roi::new(1370, 182, 150, 42);
/// メニューの見出し（「Xパワー :」「ウデマエ」）
const MENU_X_LABEL: Roi = Roi::new(1288, 156, 108, 42);
const MENU_UDEMAE_LABEL: Roi = Roi::new(1320, 148, 90, 32);
/// ルール紹介の黒いしぶきと、2 行目の語
const INTRO_SPLAT: Roi = Roi::new(600, 300, 350, 230);
const INTRO_WORD: Roi = Roi::new(640, 405, 260, 95);

/// 進行の画面のパネルの左上の、バンカラの黄色い札（「チャレンジ」「昇格戦」。手元で 0.42〜0.52、X は 0.0）
const PROGRESS_TAG: Roi = Roi::new(380, 140, 190, 55);

/// 札の黄色（縮めると縁がぼけるので、少しゆるめ）
fn tag_yellow([r, g, b]: [u8; 3]) -> bool {
    r >= 170 && g >= 170 && b <= 120
}

/// 進行の画面のモード: 黄色い札があればバンカラ、まったく無ければ X、どちらとも言えなければ None
pub fn progress_mode(img: &RgbImage) -> Option<Mode> {
    match ratio(img, PROGRESS_TAG, tag_yellow) {
        v if v >= 0.15 => Some(Mode::BankaraChallenge),
        v if v < 0.02 => Some(Mode::X),
        _ => None,
    }
}

/// 精算の画面のモード。チャレンジ・昇格戦は上に見出し（「挑戦終了!」など）と横一列の点線があり（点線のかたまり 10〜13）、
/// オープンには無い（0）。点線が無ければオープン、あればチャレンジ、どちらとも言えなければ None
pub fn udemae_mode(img: &RgbImage) -> Option<Mode> {
    let s = |v: u32| v * img.width() / 1536;
    let (x0, x1) = (s(430), s(1110));
    let runs = (s(236)..=s(256))
        .map(|y| {
            let mut n = 0;
            let mut prev = false;
            for x in x0..x1 {
                let [r, g, b] = img.get_pixel(x, y).0;
                let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
                let on = mn >= 110 && mx - mn < 25;
                if on && !prev {
                    n += 1;
                }
                prev = on;
            }
            n
        })
        .max()
        .unwrap_or(0);
    match runs {
        6.. => Some(Mode::BankaraChallenge),
        0..=1 => Some(Mode::BankaraOpen),
        _ => None,
    }
}

/// 精算のゲージ。真ん中の黒いパネルも要る（試合の始まりの「GO!」の白っぽいしぶきをゲージと見たことがある。本物のパネルは 0.94〜1.00、GO! は 0.00）
pub fn udemae_gauge(img: &RgbImage) -> bool {
    ratio(img, GAUGE, mid_gray) >= 0.6 && result_panel(img)
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

/// 白いかたまり（照合する大きさの px）
struct Blob {
    w: u32,
    h: u32,
    top: u32,
    px: u32,
}

fn blobs(img: &RgbImage, roi: Roi) -> (Vec<Blob>, u32) {
    let (x0, y0, w, h) = roi.scaled(img.width());
    let at = |x: u32, y: u32| x < img.width() && y < img.height() && white(img.get_pixel(x, y).0);
    let mut out = Vec::new();
    let mut start = None;
    for x in x0..=x0 + w {
        let any = x < x0 + w && (y0..y0 + h).any(|y| at(x, y));
        match (any, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                let ys: Vec<u32> = (y0..y0 + h).filter(|&y| (s..x).any(|xx| at(xx, y))).collect();
                let px = (s..x).map(|xx| (y0..y0 + h).filter(|&y| at(xx, y)).count() as u32).sum();
                out.push(Blob { w: x - s, h: ys.last().unwrap() - ys[0] + 1, top: ys[0] - y0, px });
                start = None;
            }
            _ => {}
        }
    }
    (out, h)
}

/// 「WIN!」「LOSE...」。手元の見本と本番の画面で、字は枠の高さの 0.58〜0.6、上端は 0.1〜0.22、
/// LOSE の点は高さ 0.12・上端 0.68。試合中の HUD やフレンドの通知は、高さがばらばら（0.2〜0.9）だった（2026-10-05）
pub fn outcome(img: &RgbImage) -> Option<Outcome> {
    let (all, rh) = blobs(img, TOP_LEFT);
    let rh = rh as f64;
    // ごく小さいもの（圧縮のちらつき・枠の端）は数えない
    let b: Vec<&Blob> = all.iter().filter(|b| b.px >= 12).collect();
    let tall = |b: &Blob| b.h as f64 >= 0.45 * rh;
    // 字は枠の中に収まっている（本物は上端から 0.1〜0.22。試合中の HUD は枠の上端にくっついて当たった）
    let good = |b: &Blob| (0.45..=0.75).contains(&(b.h as f64 / rh)) && (0.06 * rh..=0.3 * rh).contains(&(b.top as f64));
    let dot = |b: &Blob| (b.h as f64) <= 0.2 * rh && (b.w as f64) <= 0.2 * rh && b.top as f64 >= 0.55 * rh;
    if b.len() < 4 || !b[..4].iter().all(|x| tall(x)) {
        return None;
    }
    let goods: Vec<&&Blob> = b[..4].iter().filter(|x| good(x)).collect();
    // 字の上端がそろっていること（W は飾りとくっつくことがあるので、4 つのうち 3 つでよい）
    let aligned = goods.len() >= 3 && {
        let (lo, hi) = goods.iter().fold((u32::MAX, 0), |(lo, hi), x| (lo.min(x.top), hi.max(x.top)));
        hi - lo <= 4
    };
    if !aligned {
        return None;
    }
    let rest = &b[4..];
    if rest.is_empty() && (b[1].w as f64) <= 0.3 * rh {
        Some(Outcome::Win) // 2 つ目は細い「I」
    } else if rest.len() == 3 && rest.iter().all(|x| dot(x)) {
        Some(Outcome::Lose)
    } else {
        None
    }
}

/// 結果の帯のモード（X かバンカラか）
pub fn header_mode(img: &RgbImage) -> Option<Mode> {
    // 帯は写真の上の半透明の黒（手元の見本で 0.33〜0.61）。その下に結果の表の黒いパネル
    // （本物は 0.6〜0.84。試合中のマップの画面の上のプレイヤー名を帯と見間違えたときは 0.17）
    if ratio(img, HEADER_BAR, neutral_dark) < 0.3 || ratio(img, SCOREBOARD, neutral_dark) < 0.4 {
        return None;
    }
    // モード名の行。字は 170 以上を白とする（「（チャレンジ）」の小さな字は縮めると細く暗くなる）
    let (x0, y0, w, h) = MODE_ROW.scaled(img.width());
    let base = |v: u32| v * 1536 / img.width();
    let px = |x: u32, y: u32| img.get_pixel(x, y).0;
    let text = |x: u32| (y0..y0 + h).any(|y| px(x, y).iter().all(|&c| c >= 170));
    let dark = |x: u32| (y0..y0 + h).filter(|&y| neutral_dark(px(x, y))).count() * 2 > h as usize;
    let first = (x0..x0 + w).find(|&x| text(x))?;
    // 書き出しは帯の左の決まった所（手元の見本で 676〜678）
    if !(668..=690).contains(&base(first)) {
        return None;
    }
    // 字の後ろの黒い箱の右端（暗い列か字の列が続く所）。箱は X 52〜54、レギュラー 102、バンカラ 148〜150
    // （オープンとチャレンジは見分けられない）。小さな字は縮めると中間の灰色になって 4 列ほど切れるので、
    // 5 列までの切れ目は続きとみなす（箱の外の写真に入ると、もっと長く切れる）
    let gap = (5 * img.width() / 1024).max(2);
    let mut x = first;
    let mut last_in = first;
    while x < x0 + w && x - last_in <= gap {
        if dark(x) || text(x) {
            last_in = x;
        }
        x += 1;
    }
    let x = last_in + 1;
    let boxed = base(x - first);
    let width = if boxed < 250 {
        boxed
    } else {
        // 後ろも暗くて箱の端が見えない。字の幅で（X 39、バンカラ 135）
        let last = (first..x0 + w).filter(|&x| text(x)).last()?;
        base(last - first + 1) + 15
    };
    match width {
        30..=80 => Some(Mode::X),
        85..=125 => Some(Mode::Other),
        130..=200 => Some(Mode::BankaraChallenge),
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
    match mode_color(img, MATCHING_RULE, 0.08)? {
        Mode::BankaraChallenge if bankara_open(img) => Some(Mode::BankaraOpen),
        m => Some(m),
    }
}

/// バンカラのマッチングで、オープンか（チャレンジは値の右に点線の○が並ぶ（0.014〜0.026）。オープンには無い（0.000））
pub fn bankara_open(img: &RgbImage) -> bool {
    ratio(img, MATCHING_CIRCLES, light) < 0.006
}

/// ルール紹介の 1 行目が「ナワバリ」か（ナワバリバトル。2 行目は「バトル」でガチホコと同じ）。
/// 紹介の字は画面の真ん中にそろえて書かれる。ランクのルールの 1 行目は必ず「ガチ」で始まるので、濁点は 1 文字目「ガ」に付き、
/// 真ん中より左（手元で −5〜−48px）。「ナワバリ」は 3 文字目「バ」に付き、右（+34px）。フォントに頼らず、字の作りで見分ける。
/// 濁点は「同じくらいの大きさの小さな点が 2 つ並んだもの」とする（背景の白いものは対にならない）
pub fn turf_intro(img: &RgbImage) -> bool {
    let (x0, y0, w, h) = INTRO_LINE1.scaled(img.width());
    let (w, h) = (w as usize, h as usize);
    let px: Vec<bool> = (y0..y0 + h as u32)
        .flat_map(|y| (x0..x0 + w as u32).map(move |x| (x, y)))
        .map(|(x, y)| x < img.width() && y < img.height() && white(img.get_pixel(x, y).0))
        .collect();
    // 照合する大きさ（1024 幅）での px にそろえる
    let k = 1024.0 / img.width() as f64;
    let marks: Vec<Comp> = components(&px, w, h)
        .into_iter()
        .filter(|c| {
            let (cw, ch, n) = ((c.x1 - c.x0 + 1) as f64 * k, (c.y1 - c.y0 + 1) as f64 * k, c.n as f64 * k * k);
            (6.0..=15.0).contains(&ch) && (40.0..=250.0).contains(&n) && cw <= 16.0
        })
        .collect();
    let center = w as f64 / 2.0;
    marks.iter().enumerate().any(|(i, a)| {
        marks[i + 1..].iter().any(|b| {
            let (ca, cb) = ((a.x0 + a.x1) as f64 / 2.0, (b.x0 + b.x1) as f64 / 2.0);
            let pair = (a.y0 as f64 - b.y0 as f64).abs() * k <= 4.0
                && (ca - cb).abs() * k <= 16.0
                && (0.6..=1.6).contains(&(a.n as f64 / b.n as f64));
            pair && ((ca + cb) / 2.0 - center) * k > 15.0
        })
    })
}

/// つながった白いかたまり
struct Comp {
    n: usize,
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
}

fn components(px: &[bool], w: usize, h: usize) -> Vec<Comp> {
    let mut seen = vec![false; px.len()];
    let mut out = Vec::new();
    for start in 0..px.len() {
        if !px[start] || seen[start] {
            continue;
        }
        let mut c = Comp { n: 0, x0: usize::MAX, x1: 0, y0: usize::MAX, y1: 0 };
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            c.n += 1;
            c.x0 = c.x0.min(x);
            c.x1 = c.x1.max(x);
            c.y0 = c.y0.min(y);
            c.y1 = c.y1.max(y);
            let mut push = |j: usize| {
                if px[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        out.push(c);
    }
    out
}

/// 白っぽい灰色（マッチングの点線の○）
fn light([r, g, b]: [u8; 3]) -> bool {
    r.min(g).min(b) >= 150 && r.max(g).max(b) - r.min(g).min(b) < 40
}

/// メニューの見出しの色（数字の形式と合わせて使う）
pub fn menu_x_label(img: &RgbImage) -> bool {
    ratio(img, MENU_X_LABEL, teal) >= 0.05 && ratio(img, MENU_X_BOX, neutral_dark) >= 0.4
}

pub fn menu_udemae_label(img: &RgbImage) -> bool {
    ratio(img, MENU_UDEMAE_LABEL, orange) >= 0.15
        && ratio(img, MENU_UDEMAE_LABEL, teal) < 0.02
        && ratio(img, MENU_UDEMAE_BOX, neutral_dark) >= 0.4
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
/// 1 行目（「ガチ」「ガチホコ」「ナワバリ」）。ナワバリバトルも 2 行目は「バトル」なので、1 行目で見分ける
const INTRO_LINE1: Roi = Roi::new(600, 312, 340, 88);
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
fn rendered(words: &[&str]) -> Vec<Vec<Vec<bool>>> {
    let mut out = Vec::new();
    for name in ["meiryob.ttc", "YuGothB.ttc", "BIZ-UDGothicB.ttc"] {
        let Ok(bytes) = std::fs::read(format!(r"C:\Windows\Fonts\{name}")) else { continue };
        let Ok(font) = ab_glyph::FontVec::try_from_vec_and_index(bytes, 0) else { continue };
        let font = FontArc::new(font);
        let set: Option<Vec<Vec<bool>>> = words.iter().map(|w| render(&font, w)).collect();
        if let Some(set) = set {
            out.push(set);
        }
    }
    out
}

fn words() -> &'static Vec<Vec<Vec<bool>>> {
    static W: OnceLock<Vec<Vec<Vec<bool>>>> = OnceLock::new();
    W.get_or_init(|| rendered(&WORDS.map(|(_, w)| w)))
}


/// 白い字を書いた語と比べ、語ごとの似ている度合い（フォントの平均）。白が無ければ None
fn compare(img: &RgbImage, roi: Roi, sets: &[Vec<Vec<bool>>]) -> Option<Vec<f64>> {
    let (x0, y0, w, h) = roi.scaled(img.width());
    let px: Vec<bool> = (y0..y0 + h)
        .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
        .map(|(x, y)| white(img.get_pixel(x, y).0))
        .collect();
    let g = fit(&px, w as usize, h as usize)?;
    let n = sets.first()?.len();
    Some((0..n).map(|i| sets.iter().map(|s| iou(&g, &s[i])).sum::<f64>() / sets.len() as f64).collect())
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
    // 1 行目が「ナワバリ」ならナワバリバトル（2 行目は「バトル」でガチホコと同じ）
    if turf_intro(img) {
        return Some(Rule::TurfWar);
    }
    let s2 = compare(img, INTRO_WORD, words())?;
    let mut sc: Vec<(Rule, f64)> = WORDS.iter().zip(s2).map(|((r, _), v)| (*r, v)).collect();
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

/// 当たりの記録の画像で、左上のかたまりと帯を測る（`SRW_IMGS=a.jpg;b.jpg cargo test --release -- --ignored measure_imgs --nocapture`）
#[cfg(test)]
mod measure {
    use super::*;

    #[test]
    #[ignore]
    fn measure_imgs() {
        let list = std::env::var("SRW_IMGS").unwrap_or_default();
        for path in list.split(';').filter(|s| !s.is_empty()) {
            let img = crate::templates::to_work(&image::open(path).unwrap().to_rgb8());
            let (x0, y0, w, h) = TOP_LEFT.scaled(img.width());
            let at = |x: u32, y: u32| white(img.get_pixel(x, y).0);
            let mut runs = Vec::new();
            let mut start = None;
            for x in x0..=x0 + w {
                let any = x < x0 + w && (y0..y0 + h).any(|y| at(x, y));
                match (any, start) {
                    (true, None) => start = Some(x),
                    (false, Some(s)) => {
                        let ys: Vec<u32> = (y0..y0 + h).filter(|&y| (s..x).any(|xx| at(xx, y))).collect();
                        let px: usize = (s..x).map(|xx| (y0..y0 + h).filter(|&y| at(xx, y)).count()).sum();
                        runs.push(format!("x{}w{}h{}t{}p{}", s - x0, x - s, ys.last().unwrap() - ys[0] + 1, ys[0] - y0, px));
                        start = None;
                    }
                    _ => {}
                }
            }
            println!("{path}\n  左上 {} 白 {:.2}", runs.join(" "), ratio(&img, TOP_LEFT, white));
            println!("  帯 暗 {:.2} モード {:?} パネル {:.2} しぶき {:.2}", ratio(&img, HEADER_BAR, neutral_dark), header_mode(&img), ratio(&img, PANEL_EDGE, neutral_dark), ratio(&img, SPLASH, teal));
            let rec = crate::recognize::Recognizer::new(crate::templates::Templates::load(&crate::templates::Templates::default_dir()).unwrap());
            let mu = rec.read_glyphs(&img, crate::templates::place("menu_udemae_value").unwrap());
            let mx = rec.read_glyphs(&img, crate::templates::place("menu_x_value").unwrap());
            println!("  メニューの X {}/{} 見出しの色 {}", mx.text, mx.guess, menu_x_label(&img));
            for id in ["matching_x_value", "matching_udemae_value"] {
                let g = rec.read_glyphs(&img, crate::templates::place(id).unwrap());
                println!("  {id} {}/{} {}", g.text, g.guess, g.note());
            }
            let r = rec.recognize(&img);
            println!("  勝敗 {:?} メニューのウデマエ {}/{} 読み {:?}", outcome(&img), mu.text, mu.guess, r.seen);
            println!("  札 {:.3} {:?} 精算 {:?}", ratio(&img, PROGRESS_TAG, tag_yellow), progress_mode(&img), udemae_mode(&img));
            println!("  ナワバリ {} 途中 {}", turf_intro(&img), r.notes.iter().take(3).cloned().collect::<Vec<_>>().join(" / "));
        }
    }
}
