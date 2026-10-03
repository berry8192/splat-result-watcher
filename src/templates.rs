//! 登録した見本。利用者が自分の画面から登録し、`%LOCALAPPDATA%\splat-result-watcher\templates\` に置く
//! （ゲーム画面の切り抜きは配る exe に入れない。design.md）。
//!
//! 見本も照合も、ゲーム穴を **1024×576**（1280 で撮ったときのゲーム穴）にそろえてから切り出す。
//! 置き方: `templates/<pool>/<label>__<時刻>.png`（白黒の PNG。白が 255）。
//! 数字は 1 文字ずつ、大きさをそろえた 30×30 で置く（`.` は形で決まるので置かない）。

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
    Outcome,
    Mode,
    Rule,
    PowerLabel,
    /// 大きな数字（X パワー・計測完了）
    Digit,
    /// X パワーの増減（しぶきの上の小さな字）
    DigitSmall,
}

impl Pool {
    pub const ALL: [Pool; 6] = [Pool::Outcome, Pool::Mode, Pool::Rule, Pool::PowerLabel, Pool::Digit, Pool::DigitSmall];

    pub fn dir_name(self) -> &'static str {
        match self {
            Pool::Outcome => "outcome",
            Pool::Mode => "mode",
            Pool::Rule => "rule",
            Pool::PowerLabel => "power_label",
            Pool::Digit => "digit",
            Pool::DigitSmall => "digit_small",
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
    pub roi: Roi,
    /// 白黒にするしきい値（R・G・B の最小値がこれ以上なら白）
    pub min: u8,
    pub pool: Pool,
    pub kind: Kind,
}

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

/// 座標は基準 1536×864 のゲーム穴の中（docs/design.md の「見本で分かったこと」）
pub const PLACES: &[Place] = &[
    Place {
        id: "outcome",
        name: "勝敗（結果発表の左上の WIN! / LOSE...）",
        roi: Roi::new(36, 45, 220, 75),
        min: WHITE_MIN,
        pool: Pool::Outcome,
        kind: Kind::Labels(OUTCOME_LABELS),
    },
    Place {
        id: "mode",
        name: "モード（個人リザルトの右上の見出し）",
        roi: Roi::new(668, 36, 150, 26),
        min: WHITE_MIN,
        pool: Pool::Mode,
        kind: Kind::Labels(MODE_LABELS),
    },
    Place {
        id: "rule",
        name: "ルール（個人リザルトの見出し。「ガチ」の後ろ）",
        roi: Roi::new(722, 68, 66, 40),
        min: WHITE_MIN,
        pool: Pool::Rule,
        kind: Kind::Labels(RULE_LABELS),
    },
    Place {
        id: "power_label",
        name: "X パワーの画面の「Xパワー」の文字",
        roi: Roi::new(425, 440, 150, 55),
        min: WHITE_MIN,
        pool: Pool::PowerLabel,
        kind: Kind::Labels(POWER_LABEL_LABELS),
    },
    Place {
        id: "power_number",
        name: "X パワーの大きな数字",
        roi: Roi::new(590, 515, 370, 100),
        min: WHITE_MIN,
        pool: Pool::Digit,
        kind: Kind::Glyphs,
    },
    Place {
        id: "power_delta",
        name: "X パワーの増減（右のしぶきの上）",
        roi: Roi::new(965, 440, 140, 55),
        min: 140,
        pool: Pool::DigitSmall,
        kind: Kind::Glyphs,
    },
    Place {
        id: "calibrated_number",
        name: "計測完了の数字",
        roi: Roi::new(560, 460, 440, 120),
        min: WHITE_MIN,
        pool: Pool::Digit,
        kind: Kind::Glyphs,
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
            .map(|t| TemplateInfo { id: t.id.clone(), label: t.label.clone(), w: t.patch.w, h: t.patch.h })
            .collect()
    }

    /// 見本の白黒の絵（GUI で見せる）
    pub fn image(&self, pool: Pool, id: &str) -> Option<GrayImage> {
        self.get(pool).iter().find(|t| t.id == id).map(|t| patch_to_png(&t.patch))
    }

    /// 見本を足す。ファイルに書いてから持つ
    pub fn add(&mut self, pool: Pool, label: &str, patch: Patch) -> Result<String> {
        if label.is_empty() || label.contains(['/', '\\', '.']) || label.contains("__") {
            bail!("ラベル {label:?} は使えない");
        }
        let d = self.dir.join(pool.dir_name());
        std::fs::create_dir_all(&d)?;
        // 同じミリ秒に何文字も足すので、重ならない番号を付ける
        let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f").to_string();
        let mut n = 0;
        let id = loop {
            let id = format!("{label}__{stamp}{n:02}");
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

/// 照合する大きさのゲーム穴から、その場所を白黒で切り出す（見本にするもの。ずらす余白なし）
pub fn cut(work: &RgbImage, place: &Place) -> Patch {
    matching::binary_at(work, place.roi, 0, place.min)
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
        assert!(t.add(Pool::Mode, "../x", p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
