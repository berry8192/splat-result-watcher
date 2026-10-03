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

use splat_result_watcher::matching::{self, Patch, Roi};

/// ずらして探す幅（縮めた後の px）
const MARGIN: u32 = 4;
/// YouTube の画面ごと撮った見本（位置が本番と違う）。ファイル名の範囲で外す。
/// 050430 のほこは、縮尺を合わせた「（縮尺合わせ）」の方を使う
const SKIP: &[(&str, &str)] = &[
    ("20261003-035500", "20261003-035959"),
    ("20261003-050430_ほこ.png", "20261003-050430_ほこ.png"),
];
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
            ("050430", "x"),
            ("042128", "bankara_challenge"),
            ("042142", "bankara_challenge"),
        ],
    },
    Test {
        name: "ルール（個人リザルトの見出し）",
        roi: Roi::new(722, 68, 66, 40),
        templates: &[
            ("yagura", "032822"),
            ("area", "035159"),
            ("hoko", "050430"),
            ("asari", "042128"),
        ],
        answers: &[
            ("032822", "yagura"),
            ("035159", "area"),
            ("050430", "hoko"),
            ("042128", "asari"),
            ("042142", "asari"),
        ],
    },
    Test {
        name: "ルール紹介（試合の始まりの中央の大きな字）",
        roi: Roi::new(640, 405, 260, 95),
        templates: &[
            ("area", "040228"),
            ("yagura", "132222"),
            ("hoko", "041551"),
            ("asari", "040958"),
        ],
        answers: &[
            ("040228", "area"),
            ("132222", "yagura"),
            ("041551", "hoko"),
            ("040958", "asari"),
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
            !SKIP.iter().any(|(a, b)| n.as_ref() >= *a && n.as_ref() <= *b)
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
    digits(&images)?;
    Ok(())
}

/// X パワーの画面の大きな数字（x 610〜930, y 530〜600 のあたり）
const POWER: Roi = Roi::new(590, 515, 370, 100);
/// X の計測完了の数字（字が少し大きい）
const CALIBRATED: Roi = Roi::new(560, 460, 440, 120);

/// X パワーの増減（水色のしぶきの上。しぶきは少し動く）
const DELTA: Roi = Roi::new(965, 440, 140, 55);
const DELTA_MIN: u8 = 140;
const W: u8 = matching::WHITE_MIN;

/// (ファイルの時刻, ROI, 2 値のしきい値, 書いてある数字)
const NUMBERS: &[(&str, Roi, u8, &str)] = &[
    ("033034", POWER, W, "2100.0"),
    ("033044", POWER, W, "2100.0"),
    ("033100", POWER, W, "2100.0"),
    ("033253", POWER, W, "2100.0"),
    ("033306", POWER, W, "2111.0"),
    ("033316", POWER, W, "2139.4"),
    ("033345", POWER, W, "2194.6"),
    ("041847", CALIBRATED, W, "1830.4"),
    ("101412", POWER, W, "2194.6"),
    ("101431", POWER, W, "2256.8"),
    ("101601", POWER, W, "2265.9"),
    ("101625", POWER, W, "2336.8"),
    ("033306", DELTA, DELTA_MIN, "+94.6"),
    ("033316", DELTA, DELTA_MIN, "+94.6"),
    ("033345", DELTA, DELTA_MIN, "+94.6"),
    ("101412", DELTA, DELTA_MIN, "+62.2"),
    ("101431", DELTA, DELTA_MIN, "+62.2"),
    ("101601", DELTA, DELTA_MIN, "+25.0"),
    ("101625", DELTA, DELTA_MIN, "+75.0"),
];

/// 数字を 1 枚抜きで試す: 読む 1 枚以外から文字の見本を集め、その 1 枚を読む
fn digits(images: &[(String, RgbImage)]) -> Result<()> {
    println!("
## 数字（1 枚抜き: 読む 1 枚以外の見本で読む）");
    let mut cut = Vec::new();
    for (key, roi, min, text) in NUMBERS {
        let img = images
            .iter()
            .find(|(n, _)| n.contains(key))
            .map(|(_, i)| i)
            .with_context(|| format!("見本 {key} が無い"))?;
        let g = matching::glyphs(&matching::binary_at(img, *roi, 0, *min));
        if g.len() != text.chars().count() {
            println!("{key}: {} 文字に切れた（{} のはず）", g.len(), text);
        }
        cut.push((*key, *text, g));
    }

    let mut tsum = 0f64;
    for (i, (key, text, gs)) in cut.iter().enumerate() {
        // ほかの見本の文字を集める（小数点は形で決まるので要らない）
        let mut tmpl: Vec<(char, &Patch)> = Vec::new();
        for (j, (_, t, g)) in cut.iter().enumerate() {
            if i == j || g.len() != t.chars().count() {
                continue;
            }
            for (c, p) in t.chars().zip(g) {
                if let Some(p) = p {
                    tmpl.push((c, p));
                }
            }
        }
        let t0 = Instant::now();
        let mut read = String::new();
        let mut detail = Vec::new();
        for g in gs {
            let Some(g) = g else {
                read.push('.');
                continue;
            };
            // 文字ごとの一番よい一致度。1 位と 2 位を残す
            let mut best: Vec<(char, f64)> = Vec::new();
            for (c, p) in &tmpl {
                let v = matching::glyph_iou(g, p);
                match best.iter_mut().find(|b| b.0 == *c) {
                    Some(b) => b.1 = b.1.max(v),
                    None => best.push((*c, v)),
                }
            }
            best.sort_by(|a, b| b.1.total_cmp(&a.1));
            let first = best.first().copied().unwrap_or(('?', 0.0));
            let second = best.get(1).copied().unwrap_or(('?', 0.0));
            read.push(first.0);
            detail.push(format!("{}{:.2}/{}{:.2}", first.0, first.1, second.0, second.1));
        }
        tsum += t0.elapsed().as_secs_f64();
        let ok = if read == *text { "○" } else { "×" };
        println!("{ok} {key} 答え {text:<7} 読み {read:<7} [{}]", detail.join(" "));
    }
    println!(
        "1 枚あたりの照合（切り出しを除く）: {:.3}ms",
        tsum / cut.len() as f64 * 1e3
    );
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
