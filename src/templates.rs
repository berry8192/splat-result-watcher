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

/// 先頭の「=」（マイナスと同じ形に切れる）を捨てる場所。TOTAL はマイナスにならない
const STRIP_EQUALS: &[&str] = &["udemae_total"];

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
        name: "精算の TOTAL の数字（最後の p は読まない）",
        short: "TOTAL",
        roi: Roi::new(820, 318, 275, 88),
        min: WHITE_MIN,
        pool: Pool::DigitTotal,
        drop_last: true,
        bright: false,
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
pub fn cut_glyphs(work: &RgbImage, place: &Place) -> Vec<matching::Glyph> {
    let mut g = matching::glyphs(&cut(work, place));
    if STRIP_EQUALS.contains(&place.id) {
        let n = g.iter().take_while(|g| !matches!(g, matching::Glyph::Shape(_))).count();
        g.drain(..n);
    }
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
    if place.bright {
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
