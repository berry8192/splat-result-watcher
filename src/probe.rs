//! `probe`: 見本（snap）で照合を試す。2 値（IoU）とグレー（NCC）の一致度と時間を並べる。
//! 本番の撮り方（1280 で撮ってゲーム穴 1024×576）に合わせて縮めてから比べる。

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use image::imageops::{self, FilterType};
use image::RgbImage;

use crate::matching::{self, Patch, Roi};

/// 結果発表の左上の「WIN!」「LOSE...」
const OUTCOME: Roi = Roi::new(36, 45, 220, 75);
/// ずらして探す幅（縮めた後の px）
const MARGIN: u32 = 4;
/// YouTube の画面ごと撮った見本（位置が本番と違う）。名前の時刻で外す
const SKIP: (&str, &str) = ("20261003-035500", "20261003-035959");

pub fn run(args: &[String], samples: &Path) -> Result<()> {
    let mut dir = samples.join("snaps");
    let mut width = 1024u32;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = PathBuf::from(it.next().context("--dir の値が無い")?),
            "--width" => width = it.next().context("--width の値が無い")?.parse()?,
            other => bail!("知らない引数: {other}"),
        }
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("{} を読めない", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            !(n.as_ref() >= SKIP.0 && n.as_ref() <= SKIP.1)
        })
        .collect();
    files.sort();

    let load = |p: &Path| -> Result<RgbImage> {
        let img = image::open(p)
            .with_context(|| format!("{} を開けない", p.display()))?
            .to_rgb8();
        let h = width * img.height() / img.width();
        Ok(imageops::resize(&img, width, h, FilterType::Triangle))
    };
    let find = |key: &str| -> Result<RgbImage> {
        let p = files
            .iter()
            .find(|p| p.to_string_lossy().contains(key))
            .with_context(|| format!("見本 {key} が無い"))?;
        load(p)
    };
    let tmpl_img = [("WIN", find("032757")?), ("LOSE", find("034734")?)];
    let tb: Vec<Patch> = tmpl_img.iter().map(|(_, i)| matching::binary(i, OUTCOME, 0)).collect();
    let tg: Vec<Patch> = tmpl_img.iter().map(|(_, i)| matching::gray(i, OUTCOME, 0)).collect();
    println!(
        "見本: WIN 032757 / LOSE 034734、ROI {}×{} px（{}px 幅）、ずらし ±{}px",
        tb[0].w, tb[0].h, width, MARGIN
    );
    println!("{:>6} {:>6} {:>6} {:>6}  名前", "IoU勝", "IoU負", "NCC勝", "NCC負");

    let (mut tbin, mut tgray) = (0f64, 0f64);
    for p in &files {
        let img = load(p)?;
        let t0 = Instant::now();
        let sb = matching::binary(&img, OUTCOME, MARGIN);
        let ib: Vec<f64> = tb.iter().map(|t| matching::iou(t, &sb)).collect();
        let t1 = Instant::now();
        let sg = matching::gray(&img, OUTCOME, MARGIN);
        let ig: Vec<f64> = tg.iter().map(|t| matching::ncc(t, &sg)).collect();
        let t2 = Instant::now();
        tbin += (t1 - t0).as_secs_f64();
        tgray += (t2 - t1).as_secs_f64();
        let name = p.file_name().unwrap().to_string_lossy();
        println!(
            "{:>6.3} {:>6.3} {:>6.3} {:>6.3}  {}",
            ib[0], ib[1], ig[0], ig[1], name
        );
    }
    let n = files.len() as f64;
    println!(
        "{} 枚。1 枚あたり（切り出し＋見本 2 つ）: 2 値 {:.3}ms / グレー {:.3}ms",
        files.len(),
        tbin / n * 1e3,
        tgray / n * 1e3
    );
    Ok(())
}
