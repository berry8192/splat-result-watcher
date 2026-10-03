//! `serve`: 撮影 → 照合 → 状態の移り変わり → WebSocket のサーバ、をつないで動かす。
//!
//! 照合（[`recognize`]）はまだ「映像があるか」だけ。見本の登録ができたら画面ごとの照合を足す。
//! `--record` を付けると、見本の録画（`record` と同じもの）も一緒に回す。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::{Local, Utc};
use image::RgbImage;
use windows::Win32::System::Console::SetConsoleCtrlHandler;

use crate::nair::{self, Projector};
use crate::recorder::{Recorder, RecorderConfig};
use crate::server::{self, Server};
use crate::state::{Config, Machine, Seen};
use crate::{data_dir, layout, on_ctrl, open_projector, samples_dir, INTERVAL, NO_SIGNAL_DARK, STOP};

/// 撮ったゲーム穴から、何が見えたかを決める
fn recognize(game: &RgbImage) -> Seen {
    if nair::dark_ratio(game) > NO_SIGNAL_DARK {
        return Seen::NoSignal;
    }
    Seen::Unknown
}

pub fn run(args: &[String]) -> Result<()> {
    let mut addr: SocketAddr = server::DEFAULT_ADDR.parse()?;
    let mut width = 1280u32;
    let mut record = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().context(format!("{} の値が無い", a));
        match a.as_str() {
            "--addr" => addr = val()?.parse()?,
            "--width" => width = val()?.parse()?,
            "--record" => record = true,
            other => bail!("知らない指定 {}", other),
        }
    }

    nair::init_dpi();
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), true) }.context("Ctrl+C を受けられない")?;

    let log_path: PathBuf = data_dir().join("events.jsonl");
    let srv = Server::open(&log_path)?;
    println!(
        "ws://{}/events で待ち受け（出来事の控え {}、最後の seq {}）。Ctrl+C で止める",
        addr,
        log_path.display(),
        srv.last_seq()
    );
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let serving = srv.clone();
    rt.spawn(async move {
        if let Err(e) = serving.serve(addr).await {
            eprintln!("サーバが止まった: {:#}", e);
            STOP.store(true, Ordering::SeqCst);
        }
    });

    let recorder = if record {
        let r = Recorder::start(RecorderConfig {
            root: samples_dir().join("record"),
            width,
            quality: 85,
            cap_bytes: 20 * 1024 * 1024 * 1024,
        })?;
        println!("見本も残す: {}", r.session_dir.display());
        Some(r)
    } else {
        None
    };

    let mut machine = Machine::new(Config::default());
    let mut projector: Option<Projector> = None;
    let mut last_open_try: Option<Instant> = None;
    let mut next = Instant::now();
    let mut stage = "";

    while !STOP.load(Ordering::SeqCst) {
        let now = Instant::now();
        if next > now {
            std::thread::sleep((next - now).min(Duration::from_millis(100)));
            continue;
        }
        next += INTERVAL;
        if next < now {
            next = now + INTERVAL;
        }

        if projector.as_ref().is_some_and(|p| !p.alive()) {
            println!("プロジェクターが消えた。開き直す");
            projector = None;
        }
        if projector.is_none() && last_open_try.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
            last_open_try = Some(Instant::now());
            match open_projector(width) {
                Ok(p) => {
                    println!("プロジェクターを開いた");
                    projector = Some(p);
                    next = Instant::now() + Duration::from_millis(500);
                    continue;
                }
                Err(e) => println!("撮れない: {:#}", e),
            }
        }

        let seen = match projector.as_ref().map(|p| p.capture()) {
            Some(Ok(img)) => {
                let game = layout::crop_game(&img);
                let seen = recognize(&game);
                if let Some(r) = &recorder {
                    if seen != Seen::NoSignal {
                        r.push(Local::now(), game);
                    }
                }
                seen
            }
            Some(Err(e)) => {
                println!("撮影に失敗: {:#}。開き直す", e);
                projector = None;
                Seen::NoSignal
            }
            None => Seen::NoSignal,
        };

        for ev in machine.feed(Utc::now(), seen) {
            match srv.publish(ev) {
                Ok(seq) => println!("{}  出来事 #{seq}", Local::now().format("%H:%M:%S")),
                Err(e) => println!("出来事を控えられない: {:#}", e),
            }
        }
        let s = machine.stage().as_str();
        if s != stage {
            println!("{}  段階: {s}", Local::now().format("%H:%M:%S"));
            stage = s;
            srv.set_stage(s);
        }
    }
    println!("止めます");
    drop(projector);
    drop(recorder);
    rt.shutdown_timeout(Duration::from_secs(1));
    Ok(())
}
