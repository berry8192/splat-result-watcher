//! splat-result-watcher。撮影・見本集め・照合の試し・WebSocket で流す骨組み。
//!
//! - `splat-result-watcher shot [出力.png] [--width 1920]`
//!   プロジェクターを開いて 1 枚撮り、PNG で書き出す（ゲーム穴だけのものも `_game` を付けて）。
//!   10 回撮った時間も出す
//! - `splat-result-watcher snap <説明…> [--full] [--width 1920] [--dir <置き場所>]`
//!   1 枚撮ってゲーム穴を PNG で残す（見本を手で集める用）。説明はファイル名と `index.tsv` に残す
//! - `splat-result-watcher record [--dir <置き場所>] [--width 1280] [--quality 85] [--cap-gb 20] [--full]`
//!   0.5 秒ごとに撮り、ゲーム穴（layout.rs）だけを JPEG で残す（見本集め）。Ctrl+C で止める。
//!   `--width` は出力を撮る幅（撮影の時間は面積に比例し、1920 だと 20ms を超えて 1 秒ごとに落ちる）。
//!   `--full` なら出力をまるごと残す
//! - `splat-result-watcher serve [--addr 127.0.0.1:3140] [--width 1280] [--record]`
//!   撮って読み、WebSocket で流す（照合には GUI で登録した見本を使う）。`--record` で見本の録画も回す
//! - `splat-result-watcher seed-templates [--force]`
//!   手元の見本（samples/snaps）から決まった組の見本をまとめて登録する
//! - `splat-result-watcher probe [--dir <見本>] [--width 1024]`
//!   見本で照合を試す（2 値とグレーの一致度と時間。試しのためのもの）

mod probe;
mod seed;
mod serve;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::Local;
use windows::core::BOOL;
use windows::Win32::System::Console::SetConsoleCtrlHandler;

use splat_result_watcher::nair::{self, Projector};
use splat_result_watcher::recorder::{Recorder, RecorderConfig};
use splat_result_watcher::{
    layout, open_projector, samples_dir, INTERVAL, NO_SIGNAL_DARK, SLOW_CAPTURE_MS, SLOW_INTERVAL,
};

pub(crate) static STOP: AtomicBool = AtomicBool::new(false);

pub(crate) unsafe extern "system" fn on_ctrl(_: u32) -> BOOL {
    STOP.store(true, Ordering::SeqCst);
    true.into()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("shot") => shot(&args[1..]),
        Some("snap") => snap(&args[1..]),
        Some("record") => record(&args[1..]),
        Some("serve") => serve::run(&args[1..]),
        Some("seed-templates") => seed::run(&args[1..], &samples_dir()),
        Some("probe") => probe::run(&args[1..], &samples_dir()),
        _ => {
            eprintln!("使い方: splat-result-watcher shot [出力.png] [--width 1920]");
            eprintln!("        splat-result-watcher snap <説明…> [--full] [--width 1920] [--dir <置き場所>]");
            eprintln!("        splat-result-watcher record [--dir <置き場所>] [--width 1280] [--quality 85] [--cap-gb 20] [--full]");
            eprintln!("        splat-result-watcher serve [--addr 127.0.0.1:3140] [--width 1280] [--record]");
            eprintln!("        splat-result-watcher seed-templates [--force]");
            eprintln!("        splat-result-watcher probe [--dir <見本>] [--width 1024]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("エラー: {:#}", e);
        std::process::exit(1);
    }
}

fn shot(args: &[String]) -> Result<()> {
    let mut out = PathBuf::from("shot.png");
    let mut width = 1920;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--width" => width = it.next().context("--width の値が無い")?.parse()?,
            other => out = PathBuf::from(other),
        }
    }
    nair::init_dpi();
    let projector = open_projector(width)?;
    // 開いた直後は描画が間に合わず黒いことがある
    std::thread::sleep(Duration::from_millis(500));

    let mut times = Vec::new();
    let mut last = None;
    for _ in 0..10 {
        let t = Instant::now();
        let img = projector.capture()?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        last = Some(img);
    }
    let img = last.unwrap();
    img.save(&out)
        .with_context(|| format!("{} を書けない", out.display()))?;
    let game = layout::crop_game(&img);
    let game_out = out.with_file_name(format!(
        "{}_game.png",
        out.file_stem().unwrap_or_default().to_string_lossy()
    ));
    game.save(&game_out)
        .with_context(|| format!("{} を書けない", game_out.display()))?;
    let avg = times.iter().sum::<f64>() / times.len() as f64;
    let max = times.iter().cloned().fold(0.0, f64::max);
    println!(
        "{}×{}  撮影 平均 {:.1} ms / 最大 {:.1} ms  ほぼ黒 {:.0}%  -> {}（ゲーム穴 {}×{} -> {}）",
        img.width(),
        img.height(),
        avg,
        max,
        nair::dark_ratio(&img) * 100.0,
        out.display(),
        game.width(),
        game.height(),
        game_out.display()
    );
    Ok(())
}

/// 1 枚撮って、説明を付けて残す。説明はいくつの引数に分かれていてもよい（空白でつなぐ）
fn snap(args: &[String]) -> Result<()> {
    let mut dir = samples_dir().join("snaps");
    let mut width = 1920;
    let mut full = false;
    let mut words = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--full" => full = true,
            "--width" => width = it.next().context("--width の値が無い")?.parse()?,
            "--dir" => dir = PathBuf::from(it.next().context("--dir の値が無い")?),
            w => words.push(w.to_string()),
        }
    }
    let note = words.join(" ");
    if note.trim().is_empty() {
        bail!("説明が要る（例: snap 勝敗の画面 WIN）");
    }

    nair::init_dpi();
    let projector = open_projector(width)?;
    // 開いた直後は描画が間に合わず黒いことがある
    std::thread::sleep(Duration::from_millis(500));
    let at = Local::now();
    let img = projector.capture()?;
    drop(projector);
    let img = if full { img } else { layout::crop_game(&img) };

    std::fs::create_dir_all(&dir).with_context(|| format!("{} を作れない", dir.display()))?;
    let name = format!("{}_{}.png", at.format("%Y%m%d-%H%M%S"), file_safe(&note));
    let path = dir.join(&name);
    img.save(&path)
        .with_context(|| format!("{} を書けない", path.display()))?;

    // 説明の一覧（ファイル名は切り詰めるので、説明の全文はこちらに残す）
    let index = dir.join("index.tsv");
    let line = format!(
        "{}\t{}\t{}×{}\t{}\n",
        at.to_rfc3339(),
        name,
        img.width(),
        img.height(),
        note.replace(['\t', '\n', '\r'], " ")
    );
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&index)
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .with_context(|| format!("{} に書けない", index.display()))?;

    println!(
        "{}×{}  ほぼ黒 {:.0}%  -> {}",
        img.width(),
        img.height(),
        nair::dark_ratio(&img) * 100.0,
        path.display()
    );
    Ok(())
}

/// ファイル名に使えない字を _ にし、長すぎれば切る
fn file_safe(s: &str) -> String {
    let t: String = s
        .trim()
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() || c.is_whitespace() => '_',
            c => c,
        })
        .take(40)
        .collect();
    t.trim_end_matches(['.', '_']).to_string()
}

fn record(args: &[String]) -> Result<()> {
    let mut cfg = RecorderConfig {
        root: samples_dir().join("record"),
        width: 1280,
        quality: 85,
        cap_bytes: 20 * 1024 * 1024 * 1024,
    };
    let mut full = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().context(format!("{} の値が無い", a));
        match a.as_str() {
            "--full" => full = true,
            "--dir" => cfg.root = PathBuf::from(val()?),
            "--width" => cfg.width = val()?.parse()?,
            "--quality" => cfg.quality = val()?.parse()?,
            "--cap-gb" => {
                let gb: f64 = val()?.parse()?;
                cfg.cap_bytes = (gb * 1024.0 * 1024.0 * 1024.0) as u64;
            }
            other => bail!("知らない指定 {}", other),
        }
    }

    nair::init_dpi();
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), true) }.context("Ctrl+C を受けられない")?;
    println!(
        "見本の置き場所: {}（出力を {}px 幅で撮り、{}を残す・上限 {:.1} GB）。Ctrl+C で止める",
        cfg.root.display(),
        cfg.width,
        if full {
            "まるごと"
        } else {
            "ゲーム穴だけ"
        },
        cfg.cap_bytes as f64 / 1024f64.powi(3)
    );
    let width = cfg.width;
    let recorder = Recorder::start(cfg)?;
    println!("今回のフォルダ: {}", recorder.session_dir.display());

    let mut projector: Option<Projector> = None;
    let mut last_open_try: Option<Instant> = None;
    let mut interval = INTERVAL;
    let mut stats = Stats::default();
    let mut next = Instant::now();

    while !STOP.load(Ordering::SeqCst) {
        let now = Instant::now();
        if next > now {
            std::thread::sleep((next - now).min(Duration::from_millis(100)));
            continue;
        }
        next += interval;
        if next < now {
            next = now + interval; // 遅れを取り戻そうと連写しない
        }

        if projector.as_ref().is_some_and(|p| !p.alive()) {
            println!("プロジェクターが消えた。開き直す");
            projector = None;
        }
        let Some(p) = projector.as_ref() else {
            // 開けないときは 5 秒おきに試す
            if last_open_try.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
                last_open_try = Some(Instant::now());
                match open_projector(width) {
                    Ok(p) => {
                        println!("プロジェクターを開いた");
                        projector = Some(p);
                        next = Instant::now() + Duration::from_millis(500);
                    }
                    Err(e) => println!("撮れない: {:#}", e),
                }
            }
            continue;
        };

        let at = Local::now();
        let t = Instant::now();
        match p.capture() {
            Ok(img) => {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                stats.add(ms);
                let img = if full { img } else { layout::crop_game(&img) };
                if nair::dark_ratio(&img) > NO_SIGNAL_DARK {
                    stats.dark += 1;
                } else if recorder.push(at, img) {
                    stats.saved += 1;
                } else {
                    stats.dropped += 1;
                }
            }
            Err(e) => {
                println!("撮影に失敗: {:#}。開き直す", e);
                projector = None;
            }
        }

        if stats.since.elapsed() >= Duration::from_secs(60) {
            let avg = stats.avg();
            println!(
                "{}  撮影 {} 回（平均 {:.1} ms / 最大 {:.1} ms）保存 {}・真っ黒 {}・書き出しが詰まって捨てた {}",
                Local::now().format("%H:%M:%S"),
                stats.count,
                avg,
                stats.max,
                stats.saved,
                stats.dark,
                stats.dropped
            );
            if interval == INTERVAL && avg > SLOW_CAPTURE_MS {
                interval = SLOW_INTERVAL;
                println!("撮影が重いので 1 秒ごとに落とす");
            }
            stats = Stats::default();
        }
    }
    println!("止めます");
    drop(projector);
    drop(recorder); // 残りを書き切る
    Ok(())
}

struct Stats {
    since: Instant,
    count: u32,
    total_ms: f64,
    max: f64,
    saved: u32,
    dark: u32,
    dropped: u32,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            since: Instant::now(),
            count: 0,
            total_ms: 0.0,
            max: 0.0,
            saved: 0,
            dark: 0,
            dropped: 0,
        }
    }
}

impl Stats {
    fn add(&mut self, ms: f64) {
        self.count += 1;
        self.total_ms += ms;
        self.max = self.max.max(ms);
    }

    fn avg(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_ms / self.count as f64
        }
    }
}
