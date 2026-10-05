//! 登録した見本。利用者が自分の画面から登録し、`%LOCALAPPDATA%\splat-result-watcher\templates\` に置く
//! （ゲーム画面の切り抜きは配る exe に入れない。design.md）。
//!
//! 見本も照合も、ゲーム穴を **1024×576**（1280 で撮ったときのゲーム穴）にそろえてから切り出す。
//! 置き方: `templates/<pool>/<label>__<時刻>.png`（白黒の PNG。白が 255）。
//! 数字は 1 文字ずつ、大きさをそろえた 30×30 で置く（`.` と `-` は形で決まるので置かない）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::imageops::{self, FilterType};
use image::{GrayImage, Luma, RgbImage};
use serde::Serialize;

use crate::matching::{self, Patch, Roi, WHITE_MIN};

/// 照合する大きさ（ゲーム穴）
pub const WORK_W: u32 = 1024;
pub const WORK_H: u32 = 576;

/// ゲーム穴を照合する大きさにそろえる
pub fn to_work(game: &RgbImage) -> RgbImage {
    if game.width() == WORK_W && game.height() == WORK_H {
        return game.clone();
    }
    imageops::resize(game, WORK_W, WORK_H, FilterType::Triangle)
}

/// 見本の種類（どの見本の山に入れるか）
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pool {
    /// マッチング中の画面の「Xパワー」「ウデマエ」（モード）
    Matching,
    /// 試合の始まりのルール紹介
    RuleIntro,
    Outcome,
    Mode,
    Rule,
    PowerLabel,
    /// 大きな数字（X パワー・計測完了）
    Digit,
    /// X パワーの増減（しぶきの上の小さな字）
    DigitSmall,
    /// バンカラの精算の見出し（挑戦終了・勝ちぬけ・昇格）
    UdemaeTitle,
    /// バンカラの精算のゲージの下の今のポイント（小さな字）
    DigitGauge,
    /// バンカラの精算の TOTAL の数字（X パワーの数字と字体が少し違い、一致度が 0.8 止まりだった）
    DigitTotal,
    /// 試合後の進行の画面の「WIN LOSE」の見出し
    ProgressLabel,
    /// ロビーのメニューの「Xパワー :」の見出し（水色）
    MenuX,
    /// ロビーのメニューの「ウデマエ」の見出し（橙）
    MenuUdemae,
    /// ロビーのメニューの数字（X パワー・ウデマエポイント。色つきの少し細い字）
    DigitMenu,
}

impl Pool {
    pub const ALL: [Pool; 15] = [
        Pool::Matching,
        Pool::RuleIntro,
        Pool::Outcome,
        Pool::Mode,
        Pool::Rule,
        Pool::PowerLabel,
        Pool::Digit,
        Pool::DigitSmall,
        Pool::UdemaeTitle,
        Pool::DigitGauge,
        Pool::DigitTotal,
        Pool::ProgressLabel,
        Pool::MenuX,
        Pool::MenuUdemae,
        Pool::DigitMenu,
    ];

    pub fn dir_name(self) -> &'static str {
        match self {
            Pool::Matching => "matching",
            Pool::RuleIntro => "rule_intro",
            Pool::Outcome => "outcome",
            Pool::Mode => "mode",
            Pool::Rule => "rule",
            Pool::PowerLabel => "power_label",
            Pool::Digit => "digit",
            Pool::DigitSmall => "digit_small",
            Pool::UdemaeTitle => "udemae_title",
            Pool::DigitGauge => "digit_gauge",
            Pool::DigitTotal => "digit_total",
            Pool::ProgressLabel => "progress_label",
            Pool::MenuX => "menu_x",
            Pool::MenuUdemae => "menu_udemae",
            Pool::DigitMenu => "digit_menu",
        }
    }
}

/// 切り出し方
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 決まった文字の絵。(ラベル, 表示名) から選ぶ
    Labels(&'static [(&'static str, &'static str)]),
    /// 数字を 1 文字ずつ
    Glyphs,
}

/// 見本を切り出す場所（画面のどこを、どの山に）
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub id: &'static str,
    pub name: &'static str,
    /// 途中経過に出す短い名前
    pub short: &'static str,
    pub roi: Roi,
    /// 白黒にするしきい値（R・G・B の最小値がこれ以上なら白）
    pub min: u8,
    pub pool: Pool,
    pub kind: Kind,
    /// 数字の最後の 1 文字（「130p」の p）を読まずに捨てる
    pub drop_last: bool,
    /// 白黒をいちばん明るい色で決める（色つきの字。ふつうは R・G・B のどれもが明るい所だけ白）
    pub bright: bool,
}

/// 橙の字だけを読む場所（白い字の文の中にある）
const ORANGE_ONLY: &[&str] = &["fee_amount"];

/// 「=」（背の低い記号に切れる）より右だけを読む場所。精算の増減は、チャレンジの TOTAL は左にイカの印が並び、
/// オープンの 1 試合ぶんは左にずれてマイナスにもなる（「= -13p」）。「=」が無ければ読まない
const STRIP_EQUALS: &[&str] = &["udemae_total", "udemae_delta"];

pub const OUTCOME_LABELS: &[(&str, &str)] = &[("win", "WIN!"), ("lose", "LOSE...")];
pub const MODE_LABELS: &[(&str, &str)] = &[
    ("x", "Xマッチ"),
    ("bankara_challenge", "バンカラマッチ(チャレンジ)"),
    ("bankara_open", "バンカラマッチ(オープン)"),
    ("other", "その他（レギュラーマッチなど。数えない）"),
];
pub const RULE_LABELS: &[(&str, &str)] =
    &[("area", "ガチエリア"), ("yagura", "ガチヤグラ"), ("hoko", "ガチホコバトル"), ("asari", "ガチアサリ")];
pub const POWER_LABEL_LABELS: &[(&str, &str)] = &[("x_power", "「Xパワー」の見出し")];
pub const MATCHING_LABELS: &[(&str, &str)] = &[("x", "「Xパワー」（X マッチ）"), ("bankara", "「ウデマエ」（バンカラ）")];
pub const PROGRESS_LABELS: &[(&str, &str)] = &[("win_lose", "「WIN LOSE」")];
pub const MENU_X_LABELS: &[(&str, &str)] = &[("x_power", "「Xパワー :」")];
pub const MENU_UDEMAE_LABELS: &[(&str, &str)] = &[("udemae", "「ウデマエ」")];
pub const UDEMAE_TITLE_LABELS: &[(&str, &str)] =
    &[("finish", "挑戦終了!"), ("clear", "勝ちぬけ!"), ("promoted", "昇格おめでとう!!")];

/// 座標は基準 1536×864 のゲーム穴の中（docs/design.md の「見本で分かったこと」）
pub const PLACES: &[Place] = &[
    Place {
        id: "matching",
        name: "マッチング中の左のパネルの「Xパワー」「ウデマエ」（モード）",
        short: "マッチング",
        roi: Roi::new(170, 360, 110, 62),
        min: WHITE_MIN,
        pool: Pool::Matching,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(MATCHING_LABELS),
    },
    Place {
        id: "rule_intro",
        name: "ルール紹介（試合の始まりの中央の大きな字の 2 行目。ホコは「バトル」）",
        short: "ルール紹介",
        roi: Roi::new(640, 405, 260, 95),
        min: WHITE_MIN,
        pool: Pool::RuleIntro,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(RULE_LABELS),
    },
    Place {
        id: "outcome",
        name: "勝敗（結果発表の左上の WIN! / LOSE...）",
        short: "勝敗",
        roi: Roi::new(36, 45, 220, 75),
        min: WHITE_MIN,
        pool: Pool::Outcome,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(OUTCOME_LABELS),
    },
    Place {
        id: "mode",
        name: "モード（個人リザルトの右上の見出し）",
        short: "モード",
        roi: Roi::new(668, 36, 150, 26),
        min: WHITE_MIN,
        pool: Pool::Mode,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(MODE_LABELS),
    },
    Place {
        id: "rule",
        name: "ルール（個人リザルトの見出し。「ガチ」の後ろ）",
        short: "ルール",
        roi: Roi::new(722, 68, 66, 40),
        min: WHITE_MIN,
        pool: Pool::Rule,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(RULE_LABELS),
    },
    Place {
        id: "power_label",
        name: "X パワーの画面の「Xパワー」の文字",
        short: "「Xパワー」",
        roi: Roi::new(425, 440, 150, 55),
        min: WHITE_MIN,
        pool: Pool::PowerLabel,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(POWER_LABEL_LABELS),
    },
    Place {
        id: "power_number",
        name: "X パワーの大きな数字",
        short: "Xパワー",
        roi: Roi::new(590, 515, 370, 100),
        min: WHITE_MIN,
        pool: Pool::Digit,
        drop_last: false,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "power_delta",
        name: "X パワーの増減（右のしぶきの上。任意: あれば読み違いの念押しに使う）",
        short: "増減",
        roi: Roi::new(965, 440, 140, 55),
        min: 140,
        pool: Pool::DigitSmall,
        drop_last: false,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "calibrated_number",
        name: "計測完了の数字",
        short: "計測完了",
        roi: Roi::new(560, 460, 440, 120),
        min: WHITE_MIN,
        pool: Pool::Digit,
        drop_last: false,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "udemae_title",
        name: "バンカラの精算の見出し（挑戦終了! / 勝ちぬけ! / 昇格おめでとう!!）",
        short: "精算",
        roi: Roi::new(650, 200, 250, 55),
        min: WHITE_MIN,
        pool: Pool::UdemaeTitle,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(UDEMAE_TITLE_LABELS),
    },
    Place {
        id: "udemae_value",
        name: "精算のゲージの下の今のポイント（細い字。最後の p は読まない）",
        short: "ウデマエ",
        roi: Roi::new(622, 585, 80, 27),
        // 細い字なので、200 だと照合する大きさで線が途切れる。背景は暗いので下げてよい。
        // 上端は数字のすぐ上の緑の目印を外す（しきい値を下げると目印がマイナスに見える）
        min: 165,
        pool: Pool::DigitGauge,
        drop_last: true,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "udemae_total",
        name: "精算のチャレンジの TOTAL（「=」より右、最後の p は読まない）",
        short: "TOTAL",
        roi: Roi::new(740, 318, 360, 88),
        min: WHITE_MIN,
        pool: Pool::DigitTotal,
        drop_last: true,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "udemae_delta",
        name: "精算のオープンの 1 試合ぶんの増減（「= -13p」。TOTAL より上にある。「=」より右、最後の p は読まない）",
        short: "増減",
        roi: Roi::new(740, 288, 360, 84),
        // 「=」は明るい灰色（190 前後）なので、白の 200 では縁しか残らない
        min: 160,
        pool: Pool::DigitTotal,
        drop_last: true,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "fee_amount",
        name: "参加費の確かめの文の中の参加費「180p」（橙の字だけを読む。最後の p は読まない）",
        short: "参加費",
        roi: Roi::new(600, 305, 400, 36),
        min: 0,
        pool: Pool::Digit,
        drop_last: true,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "fee_value",
        name: "参加費の確かめの「現在のウデマエポイント」（黄緑の太い字。参加費を引いた後の値まで数え下がる。最後の p は読まない）",
        short: "参加費の後",
        roi: Roi::new(665, 505, 225, 85),
        min: 150,
        pool: Pool::Digit,
        drop_last: true,
        bright: true,
        kind: Kind::Glyphs,
    },
    Place {
        id: "udemae_reset",
        name: "昇格のリセットの数字「300p」（灰色の字。最後の p は読まない）",
        short: "リセット",
        roi: Roi::new(535, 598, 140, 48),
        min: 150,
        pool: Pool::Digit,
        drop_last: true,
        bright: false,
        kind: Kind::Glyphs,
    },
    Place {
        id: "menu_x_label",
        name: "ロビーのメニューの右上の「Xパワー :」（水色の字）",
        short: "メニュー X",
        roi: Roi::new(1288, 156, 108, 42),
        min: WHITE_MIN,
        pool: Pool::MenuX,
        drop_last: false,
        bright: true,
        kind: Kind::Labels(MENU_X_LABELS),
    },
    Place {
        id: "menu_x_value",
        name: "ロビーのメニューの X パワーの数字（「:」の後ろ）",
        short: "メニューの X パワー",
        roi: Roi::new(1393, 156, 110, 42),
        min: WHITE_MIN,
        pool: Pool::DigitMenu,
        drop_last: false,
        bright: true,
        kind: Kind::Glyphs,
    },
    Place {
        id: "menu_udemae_label",
        name: "ロビーのメニューの右上の「ウデマエ」（橙の小さな字）",
        short: "メニュー ウデマエ",
        roi: Roi::new(1320, 148, 90, 32),
        // 暗めの橙（赤 185 前後）の字が暗い茶色（赤 70 前後）の上にある
        min: 150,
        pool: Pool::MenuUdemae,
        drop_last: false,
        bright: true,
        kind: Kind::Labels(MENU_UDEMAE_LABELS),
    },
    Place {
        id: "menu_udemae_value",
        name: "ロビーのメニューのウデマエポイント（ランクの字の右。最後の p は読まない）",
        short: "メニューのウデマエ",
        roi: Roi::new(1375, 184, 135, 38),
        min: WHITE_MIN,
        pool: Pool::DigitMenu,
        drop_last: true,
        bright: true,
        kind: Kind::Glyphs,
    },
    Place {
        id: "matching_x_value",
        name: "マッチング中の左のパネルの X パワー（青緑の太い数字）",
        short: "マッチングの X パワー",
        roi: Roi::new(185, 432, 170, 44),
        min: 150,
        pool: Pool::Digit,
        drop_last: false,
        bright: true,
        kind: Kind::Glyphs,
    },
    Place {
        id: "matching_udemae_value",
        name: "マッチング中の左のパネルのウデマエポイント（オレンジ。先頭のランクの字と最後の p は読まない）",
        short: "マッチングのウデマエ",
        roi: Roi::new(178, 414, 160, 40),
        min: 150,
        pool: Pool::Digit,
        drop_last: true,
        bright: true,
        kind: Kind::Glyphs,
    },
    Place {
        id: "progress_label",
        name: "試合後の進行の画面の「WIN LOSE」（勝ち負けは○の判子とイカの色で数える）",
        short: "進行",
        roi: Roi::new(678, 278, 192, 34),
        min: WHITE_MIN,
        pool: Pool::ProgressLabel,
        drop_last: false,
        bright: false,
        kind: Kind::Labels(PROGRESS_LABELS),
    },
];

pub fn place(id: &str) -> Option<&'static Place> {
    PLACES.iter().find(|p| p.id == id)
}

/// 数字の 1 文字をファイル名に使えるラベルにする
pub fn glyph_label(c: char) -> Option<String> {
    match c {
        '0'..='9' => Some(c.to_string()),
        '+' => Some("plus".into()),
        '-' => Some("minus".into()),
        _ => None,
    }
}

pub fn glyph_char(label: &str) -> Option<char> {
    match label {
        "plus" => Some('+'),
        "minus" => Some('-'),
        l if l.len() == 1 && l.as_bytes()[0].is_ascii_digit() => l.chars().next(),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct Template {
    /// ファイル名（拡張子なし）。消すときに使う
    pub id: String,
    pub label: String,
    pub patch: Patch,
}

#[derive(Clone, Debug, Serialize)]
pub struct TemplateInfo {
    pub id: String,
    pub label: String,
    pub w: u32,
    pub h: u32,
    /// 自動で足した見本（learn.rs）
    pub auto: bool,
}

/// 自動で足した見本の id の印（`ラベル__auto日時`）
pub const AUTO_MARK: &str = "__auto";

/// 自動で足した見本か
pub fn is_auto(id: &str) -> bool {
    id.contains(AUTO_MARK)
}

/// 登録した見本の全部
#[derive(Clone, Debug, Default)]
pub struct Templates {
    dir: PathBuf,
    pools: BTreeMap<Pool, Vec<Template>>,
}

fn patch_to_png(p: &Patch) -> GrayImage {
    GrayImage::from_fn(p.w, p.h, |x, y| Luma([if p.px[(y * p.w + x) as usize] != 0 { 255 } else { 0 }]))
}

fn png_to_patch(img: &GrayImage) -> Patch {
    Patch {
        w: img.width(),
        h: img.height(),
        px: img.pixels().map(|p| (p.0[0] >= 128) as u8).collect(),
    }
}

impl Templates {
    pub fn default_dir() -> PathBuf {
        crate::data_dir().join("templates")
    }

    /// フォルダから全部読む（無ければ空）
    pub fn load(dir: &Path) -> Result<Self> {
        let mut pools = BTreeMap::new();
        for pool in Pool::ALL {
            let d = dir.join(pool.dir_name());
            let mut list = Vec::new();
            if d.is_dir() {
                let mut files: Vec<PathBuf> = std::fs::read_dir(&d)?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|e| e == "png"))
                    .collect();
                files.sort();
                for f in files {
                    let id = f.file_stem().unwrap().to_string_lossy().into_owned();
                    let Some((label, _)) = id.split_once("__") else {
                        continue;
                    };
                    let img = image::open(&f).with_context(|| format!("{} を読めない", f.display()))?.to_luma8();
                    list.push(Template { label: label.to_string(), patch: png_to_patch(&img), id });
                }
            }
            pools.insert(pool, list);
        }
        Ok(Templates { dir: dir.to_path_buf(), pools })
    }

    pub fn get(&self, pool: Pool) -> &[Template] {
        self.pools.get(&pool).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn list(&self, pool: Pool) -> Vec<TemplateInfo> {
        self.get(pool)
            .iter()
            .map(|t| TemplateInfo { id: t.id.clone(), label: t.label.clone(), w: t.patch.w, h: t.patch.h, auto: is_auto(&t.id) })
            .collect()
    }

    /// 見本の白黒の絵（GUI で見せる）
    pub fn image(&self, pool: Pool, id: &str) -> Option<GrayImage> {
        self.get(pool).iter().find(|t| t.id == id).map(|t| patch_to_png(&t.patch))
    }

    /// 見本を足す。ファイルに書いてから持つ
    pub fn add(&mut self, pool: Pool, label: &str, patch: Patch) -> Result<String> {
        self.add_as(pool, label, patch, "__")
    }

    /// 自動で見つけた見本を足す（id に印を付け、GUI で見分けて消せるようにする）
    pub fn add_auto(&mut self, pool: Pool, label: &str, patch: Patch) -> Result<String> {
        self.add_as(pool, label, patch, AUTO_MARK)
    }

    fn add_as(&mut self, pool: Pool, label: &str, patch: Patch, sep: &str) -> Result<String> {
        if label.is_empty() || label.contains(['/', '\\', '.']) || label.contains("__") {
            bail!("ラベル {label:?} は使えない");
        }
        let d = self.dir.join(pool.dir_name());
        std::fs::create_dir_all(&d)?;
        // 同じミリ秒に何文字も足すので、重ならない番号を付ける
        let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f").to_string();
        let mut n = 0;
        let id = loop {
            let id = format!("{label}{sep}{stamp}{n:02}");
            if !d.join(format!("{id}.png")).exists() {
                break id;
            }
            n += 1;
        };
        let path = d.join(format!("{id}.png"));
        patch_to_png(&patch).save(&path).with_context(|| format!("{} に書けない", path.display()))?;
        self.pools.entry(pool).or_default().push(Template { id: id.clone(), label: label.to_string(), patch });
        Ok(id)
    }

    pub fn remove(&mut self, pool: Pool, id: &str) -> Result<()> {
        if id.contains(['/', '\\']) || id.contains("..") {
            bail!("id {id:?} は使えない");
        }
        let path = self.dir.join(pool.dir_name()).join(format!("{id}.png"));
        if path.exists() {
            std::fs::remove_file(&path).with_context(|| format!("{} を消せない", path.display()))?;
        }
        if let Some(list) = self.pools.get_mut(&pool) {
            list.retain(|t| t.id != id);
        }
        Ok(())
    }
}

/// その場所の数字を 1 文字ずつ切る（`drop_last` なら最後の字＝単位の p を捨てる）
/// 左端にかかった別のもの（メニューのランク「S+1」の切れ端）を捨てる場所
const TRIM_LEFT_CUT: [&str; 1] = ["menu_udemae_value"];

/// 白い列が左端から始まり、その後に大きな隙間（高さの 3 割以上）があれば、隙間より左を消す。
/// ランクが「S+1」のように長いと切れ端が入る（本番で 11 列の隙間。数字の間の隙間は 7 列まで、数字は左端から離れて始まる）
fn trim_left_cut(mut p: Patch) -> Patch {
    let col = |p: &Patch, x: u32| (0..p.h).any(|y| p.px[(y * p.w + x) as usize] != 0);
    if p.w == 0 || !(col(&p, 0) || (p.w > 1 && col(&p, 1))) {
        return p;
    }
    let (mut best, mut gap_start, mut x) = (None, None, 0);
    while x < p.w {
        match (col(&p, x), gap_start) {
            (false, None) => gap_start = Some(x),
            (true, Some(s)) => {
                if best.is_none_or(|(bs, be): (u32, u32)| x - s > be - bs) {
                    best = Some((s, x));
                }
                gap_start = None;
            }
            _ => {}
        }
        x += 1;
    }
    if let Some((s, e)) = best {
        if (e - s) as f64 >= 0.3 * p.h as f64 {
            for y in 0..p.h {
                for x in 0..e {
                    p.px[(y * p.w + x) as usize] = 0;
                }
            }
        }
    }
    p
}

/// 先頭にランクの字（S・A+ など）がある場所。先頭のかたまりを捨て、次が数字の高さでなければ（「S+」の「+」など）読まない
const DROP_RANK: [&str; 1] = ["matching_udemae_value"];

/// 先頭のかたまり（ランクの字）を消す。続く低いかたまり（「S+1」の「+」と小さな「1」）も消す。
/// ただし平たいもの（高さが一番高い字の 3 割未満）はポイントのマイナスなので残す。
/// 最初の数字の高さのかたまりまでに、どちらでもないものがあれば全部消す（読み違えるより読まない）
fn drop_rank(mut p: Patch) -> Patch {
    let col = |p: &Patch, x: u32| (0..p.h).any(|y| p.px[(y * p.w + x) as usize] != 0);
    let mut runs = Vec::new();
    let mut start = None;
    for x in 0..=p.w {
        match (x < p.w && col(&p, x), start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                let rows: Vec<u32> = (0..p.h).filter(|&y| (s..x).any(|xx| p.px[(y * p.w + xx) as usize] != 0)).collect();
                runs.push((s, x, rows.last().unwrap() - rows[0] + 1));
                start = None;
            }
            _ => {}
        }
    }
    let tallest = runs.iter().map(|r| r.2).max().unwrap_or(0);
    let mut clear_to = p.w;
    for r in runs.iter().skip(1) {
        if r.2 * 10 >= tallest * 8 || r.2 * 10 < tallest * 3 {
            // 数字の高さか、平たいマイナス: ここから読む
            clear_to = r.0;
            break;
        }
    }
    for y in 0..p.h {
        for x in 0..clear_to {
            p.px[(y * p.w + x) as usize] = 0;
        }
    }
    p
}

/// 一番右の「=」（上下に離れた 2 本の横棒）より右だけを残す。「=」が無ければ None
fn after_equals(p: &Patch) -> Option<Patch> {
    let white = |x: u32, y: u32| p.px[(y * p.w + x) as usize] != 0;
    let mut cut_at = None;
    let mut x = 0;
    while x < p.w {
        if !(0..p.h).any(|y| white(x, y)) {
            x += 1;
            continue;
        }
        let x0 = x;
        while x < p.w && (0..p.h).any(|y| white(x, y)) {
            x += 1;
        }
        // 白のある行を上下のかたまりに分ける
        let rows: Vec<bool> = (0..p.h).map(|y| (x0..x).any(|xx| white(xx, y))).collect();
        let mut bands = Vec::new();
        let mut y = 0;
        while y < p.h as usize {
            if rows[y] {
                let y0 = y;
                while y < p.h as usize && rows[y] {
                    y += 1;
                }
                bands.push((y0, y));
            } else {
                y += 1;
            }
        }
        let w = (x - x0) as usize;
        if let [(a0, a1), (b0, b1)] = bands[..] {
            // 棒は横長で、間は棒の太さくらい空く
            let (ha, hb, gap) = (a1 - a0, b1 - b0, b0 - a1);
            if w >= 2 * ha.max(hb) && gap * 3 >= ha.min(hb) && gap <= 3 * ha.max(hb) {
                cut_at = Some(x);
            }
        }
    }
    let x0 = cut_at?;
    Some(Patch {
        w: p.w - x0,
        h: p.h,
        px: (0..p.h).flat_map(|y| (x0..p.w).map(move |x| (x, y))).map(|(x, y)| p.px[(y * p.w + x) as usize]).collect(),
    })
}

pub fn cut_glyphs(work: &RgbImage, place: &Place) -> Vec<matching::Glyph> {
    let mut p = cut(work, place);
    if TRIM_LEFT_CUT.contains(&place.id) {
        p = trim_left_cut(p);
    }
    if DROP_RANK.contains(&place.id) {
        p = drop_rank(p);
    }
    if STRIP_EQUALS.contains(&place.id) {
        match after_equals(&p) {
            Some(q) => p = q,
            None => return Vec::new(),
        }
    }
    let mut g = matching::glyphs(&p);
    if place.drop_last {
        if let Some(i) = g.iter().rposition(|g| matches!(g, matching::Glyph::Shape(_))) {
            g.truncate(i);
        }
    }
    g
}

/// 照合する大きさのゲーム穴から、その場所を白黒で切り出す（見本にするもの。ずらす余白なし）
pub fn cut(work: &RgbImage, place: &Place) -> Patch {
    cut_margin(work, place, 0)
}

/// ずらして探す余白を付けて切り出す（照合するもの）
pub fn cut_margin(work: &RgbImage, place: &Place, margin: u32) -> Patch {
    if ORANGE_ONLY.contains(&place.id) {
        matching::binary_orange(work, place.roi, margin)
    } else if place.bright {
        matching::binary_bright(work, place.roi, margin, place.min)
    } else {
        matching::binary_at(work, place.roi, margin, place.min)
    }
}

/// 足りない見本を、人が読む形で並べる
pub fn gaps(t: &Templates) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for p in PLACES {
        if seen.contains(&p.pool) {
            continue;
        }
        seen.push(p.pool);
        let have: Vec<&str> = t.get(p.pool).iter().map(|t| t.label.as_str()).collect();
        let missing: Vec<String> = match p.kind {
            Kind::Labels(labels) => labels
                .iter()
                // オープンは当面対応しない
                .filter(|(id, _)| *id != "bankara_open")
                .filter(|(id, _)| !have.contains(id))
                .map(|(id, name)| if *id == "other" { format!("{name}（あると安心）") } else { name.to_string() })
                .collect(),
            Kind::Glyphs => {
                let need = if p.pool == Pool::DigitSmall { "0123456789+" } else { "0123456789" };
                need.chars()
                    .filter(|c| glyph_label(*c).is_some_and(|l| !have.contains(&l.as_str())))
                    .map(|c| c.to_string())
                    .collect()
            }
        };
        // 増減の数字は念押しに使うだけなので、無くても「足りない」とは言わない（2026-10-04 ユーザー判断）
        if !missing.is_empty() && p.pool != Pool::DigitSmall {
            let name = match p.pool {
                Pool::Matching => "マッチング",
                Pool::UdemaeTitle => "精算の見出し",
                Pool::DigitGauge => "精算の小さな数字",
                Pool::DigitTotal => "精算の TOTAL の数字",
                Pool::ProgressLabel => "進行の「WIN LOSE」",
                Pool::MenuX => "メニューの「Xパワー :」",
                Pool::MenuUdemae => "メニューの「ウデマエ」",
                Pool::DigitMenu => "メニューの数字",
                Pool::RuleIntro => "ルール紹介",
                Pool::Outcome => "勝敗",
                Pool::Mode => "モード",
                Pool::Rule => "ルール（見出し）",
                Pool::PowerLabel => "「Xパワー」の見出し",
                Pool::Digit => "大きな数字",
                Pool::DigitSmall => "増減の数字",
            };
            out.push(format!("{name}: {}", missing.join("・")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_load_remove() {
        let dir = std::env::temp_dir().join(format!("srw-tmpl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = Templates::load(&dir).unwrap();
        let p = Patch { w: 3, h: 2, px: vec![1, 0, 1, 0, 1, 0] };
        let a = t.add(Pool::Digit, "7", p.clone()).unwrap();
        let b = t.add(Pool::Digit, "7", p.clone()).unwrap();
        assert_ne!(a, b);
        let t2 = Templates::load(&dir).unwrap();
        assert_eq!(t2.get(Pool::Digit).len(), 2);
        assert_eq!(t2.get(Pool::Digit)[0].patch.px, p.px);
        assert_eq!(t2.get(Pool::Digit)[0].label, "7");
        t.remove(Pool::Digit, &a).unwrap();
        assert_eq!(Templates::load(&dir).unwrap().get(Pool::Digit).len(), 1);
        assert!(t.add(Pool::Mode, "../x", p.clone()).is_err());
        let g = gaps(&t);
        assert!(g.iter().any(|l| l.starts_with("大きな数字:") && !l.contains('7') && l.contains('8')), "{g:?}");
        assert!(g.iter().any(|l| l.starts_with("モード:") && l.contains("その他")), "{g:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 登録した見本で、場所の違う数字どうしが同じ形かを比べる（`cargo test --release -- --ignored fonts --nocapture`）
#[cfg(test)]
mod fonts {
    use super::*;

    #[test]
    #[ignore]
    fn compare_digit_pools() {
        let t = Templates::load(&Templates::default_dir()).unwrap();
        let pools = [Pool::Digit, Pool::DigitSmall, Pool::DigitTotal, Pool::DigitGauge];
        // 同じ字の見本どうしの一番高い一致度（同じ山の中の別の見本どうしも、目安として出す）
        for c in "0123456789".chars() {
            let l = c.to_string();
            let mut line = format!("{c}:");
            for (i, a) in pools.iter().enumerate() {
                for b in &pools[i..] {
                    let mut best: Option<f64> = None;
                    for ta in t.get(*a).iter().filter(|t| t.label == l) {
                        for tb in t.get(*b).iter().filter(|t| t.label == l) {
                            if ta.id == tb.id {
                                continue;
                            }
                            let v = matching::glyph_iou(&ta.patch, &tb.patch);
                            best = Some(best.map_or(v, |x: f64| x.max(v)));
                        }
                    }
                    if let Some(v) = best {
                        line += &format!("  {}×{} {:.2}", a.dir_name(), b.dir_name(), v);
                    }
                }
            }
            println!("{line}");
        }
        // 違う字どうしの一番高い一致度（同じ山の中）。これより十分高ければ「同じ形」とみなせる
        for p in pools {
            let mut worst: f64 = 0.0;
            for a in t.get(p) {
                for b in t.get(p) {
                    if a.label != b.label {
                        worst = worst.max(matching::glyph_iou(&a.patch, &b.patch));
                    }
                }
            }
            println!("{} の中の、違う字どうしの一番高い一致度: {worst:.2}", p.dir_name());
        }
    }
}

/// 大きな数字の見本だけで、増減・TOTAL の数字を読めるか（`cargo test --release -- --ignored big_reads_small --nocapture`）
#[cfg(test)]
mod big_for_small {
    use super::*;

    fn load(key: &str) -> RgbImage {
        let dir = crate::samples_dir().join("snaps");
        let p = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "png") && !p.to_string_lossy().ends_with("_ほこ.png"))
            .find(|p| p.file_name().unwrap().to_string_lossy().contains(key))
            .unwrap();
        to_work(&image::open(p).unwrap().to_rgb8())
    }

    #[test]
    #[ignore]
    fn big_reads_small() {
        let t = Templates::load(&Templates::default_dir()).unwrap();
        let big = t.get(Pool::Digit);
        for (pid, key, text) in [
            ("power_delta", "033306", "+94.6"),
            ("power_delta", "101412", "+62.2"),
            ("power_delta", "101431", "+62.2"),
            ("power_delta", "101601", "+25.0"),
            ("power_delta", "101625", "+75.0"),
            ("udemae_total", "040526", "25"),
            ("udemae_total", "040905", "380"),
        ] {
            let p = place(pid).unwrap();
            let g = cut_glyphs(&load(key), p);
            let mut out = Vec::new();
            for (c, g) in text.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '+').zip(g.iter()) {
                let matching::Glyph::Shape(g) = g else {
                    out.push(format!("{c}=形"));
                    continue;
                };
                if !c.is_ascii_digit() {
                    out.push(format!("{c}=(見本なし)"));
                    continue;
                }
                // 字ごとの一番よい一致度
                let mut best: Vec<(String, f64)> = Vec::new();
                for tm in big {
                    let v = matching::glyph_iou(g, &tm.patch);
                    match best.iter_mut().find(|b| b.0 == tm.label) {
                        Some(b) => b.1 = b.1.max(v),
                        None => best.push((tm.label.clone(), v)),
                    }
                }
                best.sort_by(|a, b| b.1.total_cmp(&a.1));
                let ok = if best[0].0 == c.to_string() { "○" } else { "×" };
                out.push(format!("{c}→{}{ok}{:.2}/{}{:.2}", best[0].0, best[0].1, best[1].0, best[1].1));
            }
            println!("{key} {text:<6} {}", out.join("  "));
        }
    }
}

#[cfg(test)]
mod measure_runs {
    #[test]
    #[ignore]
    fn menu_udemae_runs() {
        let list = std::env::var("SRW_IMGS").unwrap_or_default();
        for path in list.split(';').filter(|s| !s.is_empty()) {
            let img = super::to_work(&image::open(path).unwrap().to_rgb8());
            let p = super::cut(&img, super::place("menu_udemae_value").unwrap());
            let col = |x: u32| (0..p.h).filter(|&y| p.px[(y * p.w + x) as usize] != 0).count();
            let s: String = (0..p.w).map(|x| match col(x) { 0 => '.', n if n < 5 => ':', _ => '#' }).collect();
            println!("{path} h{}\n{s}", p.h);
        }
    }
}
