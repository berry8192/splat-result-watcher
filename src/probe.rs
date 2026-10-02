//! `probe`: 見本（snap）で照合を試す。2 値（IoU）とグレー（NCC）の一致度と時間を並べる。
//! 本番の撮り方（1280 で撮ってゲーム穴 1024×576）に合わせて縮めてから比べる。
//!
//! 試すものごとに ROI と見本（ファイル名の時刻）と、答えが分かっている見本を持つ。
//! 答えが無い見本はすべて「どれでもない」として、一番高かったものを並べる。

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use image::imageops::{self, FilterType};
use image::RgbImage;

use crate::matching::{self, Patch, Roi};

/// ずらして探す幅（縮めた後の px）
const MARGIN: u32 = 4;
/// YouTube の画面ごと撮った見本（位置が本番と違う）。名前の時刻で外す
const SKIP: (&str, &str) = ("20261003-035500", "20261003-035959");
/// 答えの無い見本を何枚まで並べるか（一致度の高い順）
const SHOW_NEG: usize = 5;

struct Test {
    name: &'static str,
    roi: Roi,
    /// (ラベル, 見本にするファイルの時刻)
    templates: &'static [(&'static str, &'static str)],
    /// (ファイルの時刻, 答えのラベル)
    answers: &'static [(&'static str, &'static str)],
}

const TESTS: &[Test] = &[
    Test {
        name: "勝敗（結果発表の左上）",
        roi: Roi::new(36, 45, 220, 75),
        templates: &[("win", "032757"), ("lose", "034734")],
        answers: &[("032745", "win"), ("032757", "win"), ("034734", "lose")],
    },
    Test {
        name: "モード（個人リザルトの見出し）",
        roi: Roi::new(668, 36, 150, 26),
        templates: &[("x", "032822"), ("bankara_challenge", "042128")],
        answers: &[
            ("032822", "x"),
            ("035159", "x"),
            ("042128", "bankara_challenge"),
            ("042142", "bankara_challenge"),
        ],
    },
    Test {
        name: "ルール（個人リザルトの見出し）",
        roi: Roi::new(722, 68, 66, 40),
        templates: &[("yagura", "032822"), ("area", "035159"), ("asari", "042128")],
        answers: &[
            ("032822", "yagura"),
            ("035159", "area"),
            ("042128", "asari"),
            ("042142", "asari"),
        ],
    },
];

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

    let mut images = Vec::with_capacity(files.len());
    for p in &files {
        let img = image::open(p)
            .with_context(|| format!("{} を開けない", p.display()))?
            .to_rgb8();
        let h = width * img.height() / img.width();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        images.push((name, imageops::resize(&img, width, h, FilterType::Triangle)));
    }
    let find = |key: &str| -> Result<&RgbImage> {
        images
            .iter()
            .find(|(n, _)| n.contains(key))
            .map(|(_, i)| i)
            .with_context(|| format!("見本 {key} が無い"))
    };
    println!("{} 枚（{}px 幅に縮めて）、ずらし ±{}px", images.len(), width, MARGIN);

    for test in TESTS {
        let mut tb: Vec<Patch> = Vec::new();
        let mut tg: Vec<Patch> = Vec::new();
        for (_, key) in test.templates {
            tb.push(matching::binary(find(key)?, test.roi, 0));
            tg.push(matching::gray(find(key)?, test.roi, 0));
        }
        println!(
            "\n## {}  ROI {}×{} px  見本 {}",
            test.name,
            tb[0].w,
            tb[0].h,
            test.templates
                .iter()
                .map(|(l, k)| format!("{l}={k}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!(
            "{:<20} {}",
            "",
            test.templates
                .iter()
                .map(|(l, _)| format!("{:>9}", trunc(l, 9)))
                .collect::<Vec<_>>()
                .join("")
        );

        let (mut tbin, mut tgray) = (0f64, 0f64);
        let mut rows = Vec::new();
        for (name, img) in &images {
            let t0 = Instant::now();
            let sb = matching::binary(img, test.roi, MARGIN);
            let ib: Vec<f64> = tb.iter().map(|t| matching::iou(t, &sb)).collect();
            let t1 = Instant::now();
            let sg = matching::gray(img, test.roi, MARGIN);
            let ig: Vec<f64> = tg.iter().map(|t| matching::ncc(t, &sg)).collect();
            tbin += (t1 - t0).as_secs_f64();
            tgray += (Instant::now() - t1).as_secs_f64();
            let answer = test
                .answers
                .iter()
                .find(|(k, _)| name.contains(k))
                .map(|(_, l)| *l);
            rows.push((name, answer, ib, ig));
        }

        let fmt = |v: &[f64]| v.iter().map(|x| format!("{x:>9.3}")).collect::<String>();
        let label = |v: &[f64]| {
            let i = (0..v.len()).max_by(|&a, &b| v[a].total_cmp(&v[b])).unwrap();
            test.templates[i].0
        };
        for (name, answer, ib, ig) in rows.iter().filter(|r| r.1.is_some()) {
            let a = answer.unwrap();
            let ok = |v: &[f64]| if label(v) == a { "○" } else { "×" };
            println!("答え {:<14} 2値 {} {}  {}", trunc(a, 14), fmt(ib), ok(ib), short(name));
            println!("{:<19} NCC {} {}", "", fmt(ig), ok(ig));
        }
        let mut neg: Vec<_> = rows.iter().filter(|r| r.1.is_none()).collect();
        let top = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max);
        neg.sort_by(|a, b| top(&b.2).total_cmp(&top(&a.2)));
        println!("どれでもない見本（2 値の高い順に {SHOW_NEG} 枚）:");
        for (name, _, ib, ig) in neg.iter().take(SHOW_NEG) {
            println!("{:<19} 2値 {}  {}", "", fmt(ib), short(name));
            println!("{:<19} NCC {}", "", fmt(ig));
        }
        let worst_ncc = neg.iter().map(|r| top(&r.3)).fold(f64::MIN, f64::max);
        println!("どれでもない見本の最高: 2値 {:.3} / NCC {:.3}", top(&neg[0].2), worst_ncc);
        let n = images.len() as f64;
        println!(
            "1 枚あたり（切り出し＋見本 {} つ）: 2 値 {:.3}ms / グレー {:.3}ms",
            tb.len(),
            tbin / n * 1e3,
            tgray / n * 1e3
        );
    }
    Ok(())
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// ファイル名の時刻と説明の頭だけ
fn short(name: &str) -> String {
    let s = name.trim_start_matches("20261003-").trim_end_matches(".png");
    trunc(s, 30)
}
