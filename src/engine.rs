//! 撮影 → 照合 → 状態の移り変わり → WebSocket、を回す。コマンドの `serve` と GUI が使う。
//!
//! 撮影は普通のスレッドで 0.5 秒ごと（重ければ 1 秒）。サーバは渡された tokio の上で動かす。
//! GUI に見せるもの（段階・撮影の時間・最新の絵・途中経過・出来事）は [`Engine::snapshot`] で取る。

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{Local, Utc};
use image::RgbImage;
use serde::Serialize;
use serde_json::Value;

use crate::nair::Projector;
use crate::recognize::Recognizer;
use crate::recorder::{Recorder, RecorderConfig};
use crate::server::Server;
use crate::state::{Config, Machine, Seen};
use crate::templates::Templates;
use crate::{data_dir, layout, open_projector, samples_dir, INTERVAL, SLOW_CAPTURE_MS, SLOW_INTERVAL};

/// 控えておく出来事・記録の行の数
const KEEP_EVENTS: usize = 50;
const KEEP_LOG: usize = 200;

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub addr: SocketAddr,
    /// 出力を撮る幅（1280 ならゲーム穴が照合の大きさ 1024×576 にちょうどなる）
    pub width: u32,
    /// 見本の録画も回す
    pub record: bool,
    pub events_path: PathBuf,
    pub templates_dir: PathBuf,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            addr: crate::server::DEFAULT_ADDR.parse().unwrap(),
            width: 1280,
            record: false,
            events_path: data_dir().join("events.jsonl"),
            templates_dir: Templates::default_dir(),
        }
    }
}

/// GUI に見せる今の様子
#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub stage: String,
    pub projector: bool,
    /// 直近 1 分の撮影の平均（ms）
    pub capture_ms: f64,
    pub interval_ms: u64,
    pub clients: usize,
    pub last_seq: u64,
    pub addr: String,
    /// 最新のフレームで見えたもの・途中経過
    pub seen: String,
    pub notes: Vec<String>,
    pub record: bool,
    pub record_dir: Option<String>,
    pub server_error: Option<String>,
    /// 流した出来事（新しい順）
    pub events: Vec<Value>,
    /// 記録の行（新しい順）
    pub log: Vec<String>,
}

struct Shared {
    stop: AtomicBool,
    record: AtomicBool,
    snap: Mutex<Snapshot>,
    events: Mutex<VecDeque<Value>>,
    log: Mutex<VecDeque<String>>,
    /// これまでに書いた記録の行の数（上限で捨てた分も数える）
    log_count: AtomicU64,
    /// 最新のゲーム穴（撮ったまま）
    frame: Mutex<Option<RgbImage>>,
    recognizer: RwLock<Recognizer>,
    server: Server,
}

impl Shared {
    fn log(&self, line: String) {
        let line = format!("{}  {}", Local::now().format("%H:%M:%S"), line);
        let mut log = self.log.lock().unwrap();
        log.push_front(line);
        log.truncate(KEEP_LOG);
        self.log_count.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct Engine {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    /// コマンドで使うとき、表示し終えた記録の行の数
    printed: u64,
}

impl Engine {
    /// サーバと撮影を始める。サーバは `rt` の上で動く
    pub fn start(cfg: EngineConfig, rt: &tokio::runtime::Handle) -> Result<Engine> {
        let server = Server::open(&cfg.events_path)?;
        let templates = Templates::load(&cfg.templates_dir)?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            record: AtomicBool::new(cfg.record),
            snap: Mutex::new(Snapshot {
                stage: "idle".into(),
                addr: format!("ws://{}/events", cfg.addr),
                last_seq: server.last_seq(),
                ..Default::default()
            }),
            events: Mutex::new(VecDeque::new()),
            log: Mutex::new(VecDeque::new()),
            log_count: AtomicU64::new(0),
            frame: Mutex::new(None),
            recognizer: RwLock::new(Recognizer::new(templates)),
            server: server.clone(),
        });
        shared.log(format!(
            "{} で待ち受け（出来事の控え {}、最後の seq {}）",
            cfg.addr,
            cfg.events_path.display(),
            server.last_seq()
        ));

        let s = shared.clone();
        let addr = cfg.addr;
        rt.spawn(async move {
            if let Err(e) = server.serve(addr).await {
                let msg = format!("{:#}", e);
                s.log(format!("サーバが止まった: {msg}"));
                s.snap.lock().unwrap().server_error = Some(msg);
            }
        });

        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || capture_loop(&s, cfg.width))?;
        Ok(Engine { shared, thread: Some(thread), printed: 0 })
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut snap = self.shared.snap.lock().unwrap().clone();
        snap.clients = self.shared.server.clients();
        snap.last_seq = self.shared.server.last_seq();
        snap.record = self.shared.record.load(Ordering::Relaxed);
        snap.events = self.shared.events.lock().unwrap().iter().cloned().collect();
        snap.log = self.shared.log.lock().unwrap().iter().cloned().collect();
        snap
    }

    /// 最新のゲーム穴
    pub fn frame(&self) -> Option<RgbImage> {
        self.shared.frame.lock().unwrap().clone()
    }

    pub fn set_record(&self, on: bool) {
        self.shared.record.store(on, Ordering::Relaxed);
    }

    /// 見本を読み書きする（足したり消したりしたら、次のフレームからそれで照合する）
    pub fn with_recognizer<T>(&self, f: impl FnOnce(&mut Recognizer) -> T) -> T {
        f(&mut self.shared.recognizer.write().unwrap())
    }

    pub fn read_recognizer<T>(&self, f: impl FnOnce(&Recognizer) -> T) -> T {
        f(&self.shared.recognizer.read().unwrap())
    }

    /// まだ取り出していない記録の行を古い順に（コマンドで表示する）
    pub fn take_new_log(&mut self) -> Vec<String> {
        let log = self.shared.log.lock().unwrap();
        // 先頭が新しい。前回から増えた分だけ（上限で捨てた分は諦める）
        let total = self.shared.log_count.load(Ordering::Relaxed);
        let new = ((total - self.printed) as usize).min(log.len());
        self.printed = total;
        log.iter().take(new).rev().cloned().collect()
    }

    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture_loop(s: &Shared, width: u32) {
    let mut machine = Machine::new(Config::default());
    let mut projector: Option<Projector> = None;
    let mut recorder: Option<Recorder> = None;
    let mut last_open_try: Option<Instant> = None;
    let mut interval = INTERVAL;
    let mut next = Instant::now();
    let mut stage = "";
    let (mut times, mut since) = (Vec::new(), Instant::now());

    while !s.stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if next > now {
            std::thread::sleep((next - now).min(Duration::from_millis(100)));
            continue;
        }
        next += interval;
        if next < now {
            next = now + interval; // 遅れを取り戻そうと連写しない
        }

        // 録画の入り切り
        let want = s.record.load(Ordering::Relaxed);
        if want && recorder.is_none() {
            match Recorder::start(RecorderConfig {
                root: samples_dir().join("record"),
                width,
                quality: 85,
                cap_bytes: 20 * 1024 * 1024 * 1024,
            }) {
                Ok(r) => {
                    s.log(format!("見本の録画を始めた: {}", r.session_dir.display()));
                    s.snap.lock().unwrap().record_dir = Some(r.session_dir.display().to_string());
                    recorder = Some(r);
                }
                Err(e) => {
                    s.log(format!("見本の録画を始められない: {:#}", e));
                    s.record.store(false, Ordering::Relaxed);
                }
            }
        } else if !want && recorder.is_some() {
            recorder = None;
            s.log("見本の録画を止めた".into());
            s.snap.lock().unwrap().record_dir = None;
        }

        if projector.as_ref().is_some_and(|p| !p.alive()) {
            s.log("プロジェクターが消えた。開き直す".into());
            projector = None;
        }
        if projector.is_none() && last_open_try.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
            last_open_try = Some(Instant::now());
            match open_projector(width) {
                Ok(p) => {
                    s.log("プロジェクターを開いた".into());
                    projector = Some(p);
                    next = Instant::now() + Duration::from_millis(500);
                    continue;
                }
                Err(e) => s.log(format!("撮れない: {:#}", e)),
            }
        }

        let t = Instant::now();
        let reading = match projector.as_ref().map(|p| p.capture()) {
            Some(Ok(img)) => {
                times.push(t.elapsed().as_secs_f64() * 1000.0);
                let game = layout::crop_game(&img);
                let reading = s.recognizer.read().unwrap().recognize(&game);
                if let Some(r) = &recorder {
                    if reading.seen != Seen::NoSignal {
                        r.push(Local::now(), game.clone());
                    }
                }
                *s.frame.lock().unwrap() = Some(game);
                reading
            }
            Some(Err(e)) => {
                s.log(format!("撮影に失敗: {:#}。開き直す", e));
                projector = None;
                crate::recognize::Reading { seen: Seen::NoSignal, notes: Vec::new() }
            }
            None => crate::recognize::Reading { seen: Seen::NoSignal, notes: Vec::new() },
        };

        let seen_text = format!("{:?}", reading.seen);
        for ev in machine.feed(Utc::now(), reading.seen) {
            match s.server.publish(ev.clone()) {
                Ok(seq) => {
                    s.log(format!("出来事 #{seq}: {ev}"));
                    let mut evs = s.events.lock().unwrap();
                    let mut ev = ev;
                    ev["seq"] = seq.into();
                    evs.push_front(ev);
                    evs.truncate(KEEP_EVENTS);
                }
                Err(e) => s.log(format!("出来事を控えられない: {:#}", e)),
            }
        }
        let st = machine.stage().as_str();
        if st != stage {
            s.log(format!("段階: {st}"));
            stage = st;
            s.server.set_stage(st);
        }

        // 1 分ごとに撮影の時間を見て、重ければ 1 秒ごとに落とす
        if since.elapsed() >= Duration::from_secs(60) && !times.is_empty() {
            let avg = times.iter().sum::<f64>() / times.len() as f64;
            if interval == INTERVAL && avg > SLOW_CAPTURE_MS {
                interval = SLOW_INTERVAL;
                s.log(format!("撮影が重い（平均 {avg:.1}ms）ので 1 秒ごとに落とす"));
            }
            s.snap.lock().unwrap().capture_ms = avg;
            times.clear();
            since = Instant::now();
        }

        let mut snap = s.snap.lock().unwrap();
        snap.stage = st.to_string();
        snap.projector = projector.is_some();
        snap.interval_ms = interval.as_millis() as u64;
        if snap.capture_ms == 0.0 && !times.is_empty() {
            snap.capture_ms = times.iter().sum::<f64>() / times.len() as f64;
        }
        snap.seen = seen_text;
        snap.notes = reading.notes;
    }
    s.log("止めます".into());
    drop(projector);
    drop(recorder);
}
