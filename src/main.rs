//! splat-result-watcher。いまは撮影と見本集めだけ。
//!
//! - `splat-result-watcher shot [出力.png] [--width 1920]`
//!   プロジェクターを開いて 1 枚撮り、PNG で書き出す。10 回撮った時間も出す
//! - `splat-result-watcher record [--dir <置き場所>] [--width 1280] [--quality 85] [--cap-gb 20]`
//!   0.5 秒ごとに撮って JPEG で残す（見本集め）。Ctrl+C で止める。
//!   `--width` の大きさで撮る（撮影の時間は面積に比例し、1920 だと 20ms を超えて 1 秒ごとに落ちる）

mod nair;
mod recorder;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::Local;
use windows::core::BOOL;
use windows::Win32::System::Console::SetConsoleCtrlHandler;

use nair::Projector;
use recorder::{Recorder, RecorderConfig};

/// 撮る間隔。1 回の撮影が重ければ（平均 20ms 超）1 秒に落とす
const INTERVAL: Duration = Duration::from_millis(500);
const SLOW_INTERVAL: Duration = Duration::from_secs(1);
const SLOW_CAPTURE_MS: f64 = 20.0;
/// これより黒い絵は「映像が来ていない」として残さない
const NO_SIGNAL_DARK: f64 = 0.98;

static STOP: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn on_ctrl(_: u32) -> BOOL {
    STOP.store(true, Ordering::SeqCst);
    true.into()
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("splat-result-watcher")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("shot") => shot(&args[1..]),
        Some("record") => record(&args[1..]),
        _ => {
            eprintln!("使い方: splat-result-watcher shot [出力.png] [--width 1920]");
            eprintln!("        splat-result-watcher record [--dir <置き場所>] [--width 1280] [--quality 85] [--cap-gb 20]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("エラー: {:#}", e);
        std::process::exit(1);
    }
}

fn open_projector(width: u32) -> Result<Projector> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    Projector::open(Some(&dir.join("projector.hwnd")), width)
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
    let avg = times.iter().sum::<f64>() / times.len() as f64;
    let max = times.iter().cloned().fold(0.0, f64::max);
    println!(
        "{}×{}  撮影 平均 {:.1} ms / 最大 {:.1} ms  ほぼ黒 {:.0}%  -> {}",
        img.width(),
        img.height(),
        avg,
        max,
        nair::dark_ratio(&img) * 100.0,
        out.display()
    );
    Ok(())
}

fn record(args: &[String]) -> Result<()> {
    let mut cfg = RecorderConfig {
        root: data_dir().join("samples"),
        width: 1280,
        quality: 85,
        cap_bytes: 20 * 1024 * 1024 * 1024,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().context(format!("{} の値が無い", a));
        match a.as_str() {
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
        "見本の置き場所: {}（{}px 幅・上限 {:.1} GB）。Ctrl+C で止める",
        cfg.root.display(),
        cfg.width,
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
