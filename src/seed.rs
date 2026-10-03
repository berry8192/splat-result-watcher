//! `seed-templates`: 手元の見本（`samples/snaps/`、git 管理外）から、決まった組の見本をまとめて登録する。
//! 自分の画面で撮った見本を、GUI で 1 つずつ登録する手間を省くためのもの（中身は recognize.rs の試験と同じ組）。
//! 見本の置き場所はふつうの見本と同じ（`%LOCALAPPDATA%\splat-result-watcher\templates\`）。

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::RgbImage;

use splat_result_watcher::matching::Glyph;
use splat_result_watcher::templates::{self, glyph_label, place, Templates};

/// (場所, ファイル名の時刻, ラベル)
const LABELS: &[(&str, &str, &str)] = &[
    ("outcome", "032757", "win"),
    ("outcome", "034734", "lose"),
    ("mode", "032822", "x"),
    ("mode", "042128", "bankara_challenge"),
    ("rule", "032822", "yagura"),
    ("rule", "035159", "area"),
    ("rule", "042128", "asari"),
    ("rule", "050430", "hoko"),
    ("power_label", "033034", "x_power"),
    ("rule_intro", "040228", "area"),
    ("rule_intro", "132222", "yagura"),
    ("rule_intro", "041551", "hoko"),
    ("rule_intro", "040958", "asari"),
    ("udemae_title", "040440", "finish"),
    ("udemae_title", "040820", "clear"),
    ("udemae_title", "041716", "promoted"),
    ("matching", "032333", "x"),
    ("matching", "040151", "bankara"),
];

/// (場所, ファイル名の時刻, 書いてある数字)
const NUMBERS: &[(&str, &str, &str)] = &[
    ("power_number", "101431", "2256.8"),
    ("power_number", "033316", "2139.4"),
    ("power_number", "101601", "2265.9"),
    ("calibrated_number", "041847", "1830.4"),
    ("power_delta", "101601", "+25.0"),
    ("power_delta", "101431", "+62.2"),
    ("power_delta", "033306", "+94.6"),
    ("power_delta", "101625", "+75.0"),
    ("udemae_value", "040440", "130"),
    ("udemae_value", "041132", "685"),
    ("udemae_value", "040820", "-15"),
    ("udemae_value", "040905", "365"),
    ("udemae_value", "040858", "319"),
    ("udemae_total", "040526", "25"),
    ("udemae_total", "040905", "380"),
];

fn load(dir: &Path, key: &str) -> Result<RgbImage> {
    let p: PathBuf = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        // YouTube の画面ごと撮ったほこは使わず、縮尺を合わせた方を使う
        .filter(|p| !p.to_string_lossy().ends_with("_ほこ.png"))
        .find(|p| p.file_name().unwrap().to_string_lossy().contains(key))
        .with_context(|| format!("見本 {key} が {} に無い", dir.display()))?;
    Ok(templates::to_work(&image::open(&p)?.to_rgb8()))
}

pub fn run(args: &[String], samples: &Path) -> Result<()> {
    let force = args.iter().any(|a| a == "--force");
    let dir = Templates::default_dir();
    let mut t = Templates::load(&dir)?;
    let already: usize = templates::PLACES.iter().map(|p| t.get(p.pool).len()).sum();
    if already > 0 && !force {
        bail!("{} にもう見本がある（{already} 個）。足すなら --force", dir.display());
    }
    let snaps = samples.join("snaps");
    let mut n = 0;
    for (pid, key, label) in LABELS {
        let p = place(pid).context("場所が無い")?;
        t.add(p.pool, label, templates::cut(&load(&snaps, key)?, p))?;
        n += 1;
    }
    for (pid, key, text) in NUMBERS {
        let p = place(pid).context("場所が無い")?;
        let g = templates::cut_glyphs(&load(&snaps, key)?, p);
        if g.len() != text.chars().count() {
            println!("{key} の {pid} は {} 文字に切れた（{text} のはず）。飛ばす", g.len());
            continue;
        }
        for (c, g) in text.chars().zip(g) {
            if let (Some(l), Glyph::Shape(g)) = (glyph_label(c), g) {
                t.add(p.pool, &l, g)?;
                n += 1;
            }
        }
    }
    println!("{n} 個の見本を {} に登録した", dir.display());
    for g in templates::gaps(&t) {
        println!("足りない: {g}");
    }
    Ok(())
}
