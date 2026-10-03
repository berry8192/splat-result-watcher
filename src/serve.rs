//! `serve`: 撮影 → 照合 → 状態の移り変わり → WebSocket を、コマンドとして回す（中身は engine.rs）。
//! 照合には GUI で登録した見本を使う。`--record` を付けると見本の録画も一緒に回す。

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use windows::Win32::System::Console::SetConsoleCtrlHandler;

use splat_result_watcher::engine::Engine;
use splat_result_watcher::settings::Settings;
use splat_result_watcher::nair;

use crate::{on_ctrl, STOP};

pub fn run(args: &[String]) -> Result<()> {
    // 既定は GUI の設定（settings.json）。引数で上書きする
    let (settings, warn) = Settings::load();
    if let Some(w) = warn {
        println!("{w}");
    }
    let mut cfg = settings.engine_config();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().context(format!("{} の値が無い", a));
        match a.as_str() {
            "--addr" => cfg.addr = val()?.parse::<SocketAddr>()?,
            "--width" => cfg.width = val()?.parse()?,
            "--record" => cfg.record = true,
            other => bail!("知らない指定 {}", other),
        }
    }

    nair::init_dpi();
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), true) }.context("Ctrl+C を受けられない")?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut engine = Engine::start(cfg, rt.handle())?;
    println!("Ctrl+C で止める");
    while !STOP.load(Ordering::SeqCst) {
        for line in engine.take_new_log() {
            println!("{line}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    engine.stop();
    for line in engine.take_new_log() {
        println!("{line}");
    }
    rt.shutdown_timeout(Duration::from_secs(1));
    Ok(())
}
