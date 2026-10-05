//! 撮影 → 照合 → 状態の移り変わり → WebSocket、を回す。コマンドの `serve` と GUI が使う。
//!
//! 撮影は普通のスレッドで 0.5 秒ごと（重ければ 1 秒）。サーバは渡された tokio の上で動かす。
//! GUI に見せるもの（段階・撮影の時間・最新の絵・途中経過・出来事）は [`Engine::snapshot`] で取る。

use std::collections::{BTreeMap, VecDeque};
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use image::RgbImage;
use serde::Serialize;
use serde_json::Value;

use crate::learn::{LabelLearner, Learner};
use crate::nair::Projector;
use crate::recognize::Recognizer;
use crate::recorder::{Recorder, RecorderConfig};
use crate::server::Server;
use crate::state::{Config, Machine, Seen};
use crate::templates::Templates;
use crate::{data_dir, layout, nair, open_projector, samples_dir, INTERVAL, SLOW_CAPTURE_MS, SLOW_INTERVAL};

/// 控えておく出来事・記録の行の数
const KEEP_EVENTS: usize = 50;
const KEEP_LOG: usize = 200;
/// 見本の登録に使う、直近の画面を持っておく量（JPEG で 1 枚 100KB ほど）。古い方から捨てる
pub const KEEP_RECENT: Duration = Duration::from_secs(30 * 60);
pub const KEEP_RECENT_BYTES: usize = 200 * 1024 * 1024;
/// 結果を読み中・試合後は速く撮って溜める（照合と状態の移り変わりは今まで通り 0.5 秒ごと。
/// 状態は「何枚続いたか」で落ち着きを見るので、照合の間隔は変えない）
pub const FAST_INTERVAL: Duration = Duration::from_millis(200);
/// バトル中は見本に要る画面がほぼ無いので、読めなかった画面は 5 秒に 1 枚だけ溜める
const BATTLE_KEEP_EVERY: Duration = Duration::from_secs(5);

/// 直近の画面 1 枚（ゲーム穴を JPEG にしたもの）と、そのとき見えたもの
#[derive(Clone, Debug)]
pub struct RecentFrame {
    pub at: DateTime<Local>,
    pub seen: String,
    pub jpeg: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub addr: SocketAddr,
    /// 出力を撮る幅（1280 ならゲーム穴が照合の大きさ 1024×576 にちょうどなる）
    pub width: u32,
    /// 見本の録画も回す
    pub record: bool,
    /// 見本の録画の上限（バイト）
    pub record_cap_bytes: u64,
    pub events_path: PathBuf,
    pub templates_dir: PathBuf,
    /// 今の試合の控え（落ちても、リザルトまでに起動し直せば続きから読む）
    pub game_path: PathBuf,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            addr: crate::server::DEFAULT_ADDR.parse().unwrap(),
            width: 1280,
            record: false,
            record_cap_bytes: 20 * 1024 * 1024 * 1024,
            events_path: data_dir().join("events.jsonl"),
            templates_dir: Templates::default_dir(),
            game_path: data_dir().join("current_game.json"),
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
    /// 足りない見本（設定の画面と見せる窓で知らせる）
    pub template_gaps: Vec<String>,
    /// 流した出来事（新しい順）
    pub events: Vec<Value>,
    /// 記録の行（新しい順）
    pub log: Vec<String>,
}

struct Shared {
    stop: AtomicBool,
    record: AtomicBool,
    /// 今の試合を手で捨てる（次のフレームで）
    reset_game: AtomicBool,
    snap: Mutex<Snapshot>,
    events: Mutex<VecDeque<Value>>,
    log: Mutex<VecDeque<String>>,
    /// これまでに書いた記録の行の数（上限で捨てた分も数える）
    log_count: AtomicU64,
    /// 最新のゲーム穴（撮ったまま）
    frame: Mutex<Option<RgbImage>>,
    /// 直近の画面（古い順。映像なしは入れない）
    recent: Mutex<VecDeque<RecentFrame>>,
    recent_bytes: AtomicU64,
    recognizer: RwLock<Recognizer>,
    server: Server,
}

/// 記録の行をファイルにも残す（`%LOCALAPPDATA%\\splat-result-watcher\\logs\\日付.log`。終わった後で見返す）
fn append_log_file(line: &str) {
    let dir = data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.log", Local::now().format("%Y%m%d")));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

impl Shared {
    fn log(&self, line: String) {
        let line = format!("{}  {}", Local::now().format("%H:%M:%S"), line);
        append_log_file(&line);
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
            reset_game: AtomicBool::new(false),
            snap: Mutex::new(Snapshot {
                stage: "idle".into(),
                addr: format!("ws://{}/events", cfg.addr),
                last_seq: server.last_seq(),
                ..Default::default()
            }),
            // 再起動しても直近の勝敗などを見せられるよう、控えから最近の分を読んでおく
            events: Mutex::new(server.recent(KEEP_EVENTS).into()),
            log: Mutex::new(VecDeque::new()),
            log_count: AtomicU64::new(0),
            frame: Mutex::new(None),
            recent: Mutex::new(VecDeque::new()),
            recent_bytes: AtomicU64::new(0),
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
            .spawn(move || capture_loop(&s, &cfg))?;
        Ok(Engine { shared, thread: Some(thread), printed: 0 })
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut snap = self.shared.snap.lock().unwrap().clone();
        snap.clients = self.shared.server.clients();
        snap.last_seq = self.shared.server.last_seq();
        snap.record = self.shared.record.load(Ordering::Relaxed);
        snap.events = self.shared.events.lock().unwrap().iter().cloned().collect();
        snap.log = self.shared.log.lock().unwrap().iter().cloned().collect();
        snap.template_gaps = self.read_recognizer(|r| crate::templates::gaps(r.templates()));
        snap
    }

    /// 最新のゲーム穴
    pub fn frame(&self) -> Option<RgbImage> {
        self.shared.frame.lock().unwrap().clone()
    }

    /// 直近の画面（古い順）。見本の登録で、遊んだ後に戻って選ぶ
    pub fn recent(&self) -> Vec<RecentFrame> {
        self.shared.recent.lock().unwrap().iter().cloned().collect()
    }

    pub fn set_record(&self, on: bool) {
        self.shared.record.store(on, Ordering::Relaxed);
    }

    /// 今の試合を捨てて待機に戻す（止まったまま残ったとき）
    pub fn reset_game(&self) {
        self.shared.reset_game.store(true, Ordering::Relaxed);
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

/// 書きかけで落ちても壊れないよう、別の名前に書いてから差し替える
fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// 直近の画面に足し、古いものを捨てる
fn keep_recent(s: &Shared, game: &RgbImage, seen: String) {
    let mut buf = Vec::new();
    if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90).encode_image(game).is_err() {
        return;
    }
    let at = Local::now();
    let oldest = at - chrono::Duration::from_std(KEEP_RECENT).unwrap();
    let mut recent = s.recent.lock().unwrap();
    let mut bytes = s.recent_bytes.load(Ordering::Relaxed) as usize + buf.len();
    while recent.front().is_some_and(|f| f.at < oldest || bytes > KEEP_RECENT_BYTES) {
        bytes -= recent.pop_front().unwrap().jpeg.len();
    }
    s.recent_bytes.store(bytes as u64, Ordering::Relaxed);
    recent.push_back(RecentFrame { at, seen, jpeg: Arc::new(buf) });
}

fn capture_loop(s: &Shared, cfg: &EngineConfig) {
    let (width, game_path) = (cfg.width, cfg.game_path.as_path());
    let mut machine = Machine::new(Config::default());
    if let Ok(saved) = std::fs::read_to_string(game_path) {
        match machine.restore(&saved, Utc::now()) {
            Some(id) => s.log(format!("途中の試合 {id} の続きから読む")),
            None if !saved.contains("\"game\":null") => s.log("控えの試合は古いので捨てた".into()),
            None => {}
        }
    }
    let mut last_saved = machine.save();
    let mut learner = Learner::default();
    let mut label_learner = LabelLearner::default();
    let mut projector: Option<Projector> = None;
    let mut recorder: Option<Recorder> = None;
    let mut last_open_try: Option<Instant> = None;
    let mut interval = INTERVAL;
    // 今の撮る間隔（段階で変わる）。照合は `interval` ごと
    let mut tick = interval;
    let mut next = Instant::now();
    let mut last_read: Option<Instant> = None;
    let mut last_seen = String::new();
    let mut last_battle_keep: Option<Instant> = None;
    let mut stage = "";
    // 段階ごとの、場所ごとの一番高い一致度（段階が変わるたびに記録へ書いて空にする）
    let mut peaks: BTreeMap<String, (String, f64)> = BTreeMap::new();
    let (mut times, mut since) = (Vec::new(), Instant::now());

    while !s.stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if next > now {
            std::thread::sleep((next - now).min(Duration::from_millis(100)));
            continue;
        }
        next += tick;
        if next < now {
            next = now + tick; // 遅れを取り戻そうと連写しない
        }

        // 録画の入り切り
        let want = s.record.load(Ordering::Relaxed);
        if want && recorder.is_none() {
            match Recorder::start(RecorderConfig {
                root: samples_dir().join("record"),
                width,
                quality: 85,
                cap_bytes: cfg.record_cap_bytes,
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
            // 開くのに十数秒かかって失敗することがある。5 秒は試し終わってから数える（その間に映像なしを流す）
            let opened = open_projector(width);
            last_open_try = Some(Instant::now());
            match opened {
                Ok(p) => {
                    s.log("プロジェクターを開いた".into());
                    projector = Some(p);
                    next = Instant::now() + Duration::from_millis(500);
                    continue;
                }
                Err(e) => s.log(format!("撮れない: {:#}", e)),
            }
        }

        if s.reset_game.swap(false, Ordering::Relaxed) {
            match machine.drop_game() {
                Some(id) => s.log(format!("試合 {id} を手で捨てた")),
                None => s.log("捨てる試合は無かった".into()),
            }
        }

        // 速く撮っている間は、照合の間隔が来たフレームだけを読む（間のフレームは溜めるだけ）
        let read_now = last_read.is_none_or(|t| t.elapsed() + tick / 2 >= interval);
        let in_battle = stage == crate::state::Stage::InBattle.as_str();
        let t = Instant::now();
        let reading = match projector.as_ref().map(|p| p.capture()) {
            Some(Ok(img)) if !read_now => {
                let game = layout::crop_game(&img);
                if !in_battle && nair::dark_ratio(&game) <= crate::NO_SIGNAL_DARK {
                    keep_recent(s, &game, last_seen.clone());
                }
                *s.frame.lock().unwrap() = Some(game);
                continue;
            }
            Some(Ok(img)) => {
                times.push(t.elapsed().as_secs_f64() * 1000.0);
                last_read = Some(Instant::now());
                let game = layout::crop_game(&img);
                let reading = s.recognizer.read().unwrap().recognize(&game);
                if let Some(r) = &recorder {
                    if reading.seen != Seen::NoSignal {
                        r.push(Local::now(), game.clone());
                    }
                }
                let seen = format!("{:?}", reading.seen);
                // バトル中は、何か読めた画面（ルール紹介・無効試合の札など）と、5 秒に 1 枚だけ
                let keep = match reading.seen {
                    Seen::NoSignal => false,
                    Seen::Unknown if in_battle => last_battle_keep.is_none_or(|t| t.elapsed() >= BATTLE_KEEP_EVERY),
                    _ => true,
                };
                if keep {
                    if in_battle {
                        last_battle_keep = Some(Instant::now());
                    }
                    keep_recent(s, &game, seen.clone());
                }
                last_seen = seen;
                *s.frame.lock().unwrap() = Some(game);
                reading
            }
            Some(Err(e)) => {
                s.log(format!("撮影に失敗: {:#}。開き直す", e));
                projector = None;
                crate::recognize::Reading::no_signal()
            }
            None => crate::recognize::Reading::no_signal(),
        };

        let seen_text = format!("{:?}", reading.seen);
        for (place, label, score) in &reading.peaks {
            let e = peaks.entry(place.clone()).or_insert_with(|| (label.clone(), *score));
            if *score > e.1 {
                *e = (label.clone(), *score);
            }
        }
        // 読めない字を、ほかの確かな数字から埋められたら見本に足す（確度の高いものだけ。learn.rs）
        // 形と色で見分けた見出しも、別の画面で確かめられたら見本に足す
        let learned = {
            let r = s.recognizer.read().unwrap();
            let now = Utc::now();
            let mut l = learner.feed(now, &reading.seen, &reading.numbers, r.templates());
            l.extend(label_learner.feed(now, &reading.seen, &reading.shape_labels, r.templates()));
            l
        };
        if !learned.is_empty() {
            let mut r = s.recognizer.write().unwrap();
            for l in learned {
                match r.templates_mut().add_auto(l.pool, &l.label, l.patch) {
                    Ok(_) => s.log(format!("見本を自動で足した: {} の「{}」。{}", l.pool.dir_name(), l.label, l.why)),
                    Err(e) => s.log(format!("見本を自動で足せない: {e:#}")),
                }
            }
        }
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
        let saved = machine.save();
        if saved != last_saved {
            if let Err(e) = write_atomic(game_path, &saved) {
                s.log(format!("今の試合を控えられない: {e}"));
            }
            last_saved = saved;
        }
        let st = machine.stage().as_str();
        if st != stage {
            if !peaks.is_empty() {
                let list: Vec<String> = peaks.iter().map(|(p, (l, v))| format!("{p} {l} {v:.2}")).collect();
                s.log(format!("{} の間の一番高い一致度: {}", if stage.is_empty() { "起動" } else { stage }, list.join(" / ")));
                peaks.clear();
            }
            s.log(format!("段階: {st}"));
            stage = st;
            s.server.set_stage(st);
        }
        // 結果を読み中・試合後は、見本に要る画面が続くので速く撮る（撮影が重いときはしない）
        let fast = matches!(machine.stage(), crate::state::Stage::Reading | crate::state::Stage::PostMatch);
        tick = if fast && interval == INTERVAL { FAST_INTERVAL } else { interval };

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
