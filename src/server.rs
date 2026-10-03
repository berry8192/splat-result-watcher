//! WebSocket のサーバ（docs/protocol.md の「つなぎ方」「取りこぼしと二重計上」）。
//!
//! - 既定は `ws://127.0.0.1:3140/events`。クライアントは `?after=<最後に受けた seq>` でつなぐ
//! - つながったら `hello` → `seq > after` の出来事を古い順に → 今の `status`、以降は起きた順に流す
//! - 少なくとも 5 秒ごとに `status` を送る
//! - 出来事は 1 行 1 つでファイルに控える。`seq` はその続きから振るので、再起動しても戻らない
//!
//! 撮影の繰り返しは普通のスレッドで回すので、[`Server::publish`] / [`Server::set_stage`] は同期で呼べる。

use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

pub const DEFAULT_ADDR: &str = "127.0.0.1:3140";
const BEAT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Server {
    inner: Arc<Inner>,
}

struct Inner {
    log: Mutex<Log>,
    tx: broadcast::Sender<String>,
    path: PathBuf,
}

struct Log {
    /// (seq, 送る文字列)。控えのファイルを丸ごと持つ（1 試合数件なので小さい）
    events: Vec<(u64, String)>,
    last_seq: u64,
    stage: &'static str,
}

#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: u64,
}

fn status_json(stage: &str) -> String {
    json!({"type": "status", "state": stage}).to_string()
}

impl Server {
    /// 控えのファイルを読んで用意する（無ければ空から）
    pub fn open(path: &Path) -> Result<Self> {
        let mut events = Vec::new();
        if path.exists() {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("{} を読めない", path.display()))?;
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                // 壊れた行（書きかけで落ちた）は飛ばす
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if let Some(seq) = v["seq"].as_u64() {
                    events.push((seq, line.to_string()));
                }
            }
        }
        let last_seq = events.iter().map(|e| e.0).max().unwrap_or(0);
        let (tx, _) = broadcast::channel(256);
        Ok(Server {
            inner: Arc::new(Inner {
                log: Mutex::new(Log { events, last_seq, stage: "idle" }),
                tx,
                path: path.to_path_buf(),
            }),
        })
    }

    pub fn last_seq(&self) -> u64 {
        self.inner.log.lock().unwrap().last_seq
    }

    /// 出来事に seq を振って控え、つながっている相手へ流す
    pub fn publish(&self, mut body: Value) -> Result<u64> {
        let mut log = self.inner.log.lock().unwrap();
        let seq = log.last_seq + 1;
        body["seq"] = seq.into();
        let text = body.to_string();
        if let Some(dir) = self.inner.path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.inner.path)
            .and_then(|mut f| writeln!(f, "{text}"))
            .with_context(|| format!("{} に書けない", self.inner.path.display()))?;
        log.last_seq = seq;
        log.events.push((seq, text.clone()));
        let _ = self.inner.tx.send(text);
        Ok(seq)
    }

    /// 段階を変える。変わったときだけすぐに流す（変わらなくても 5 秒ごとに送る）
    pub fn set_stage(&self, stage: &'static str) {
        let mut log = self.inner.log.lock().unwrap();
        if log.stage != stage {
            log.stage = stage;
            let _ = self.inner.tx.send(status_json(stage));
        }
    }

    /// 待ち受けを始める。止まるまで返らない
    pub async fn serve(self, addr: SocketAddr) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("{addr} を開けない（ほかのアプリが使っている？）"))?;
        self.serve_on(listener).await
    }

    pub async fn serve_on(self, listener: tokio::net::TcpListener) -> Result<()> {
        let router = Router::new().route("/events", get(upgrade)).with_state(self);
        axum::serve(listener, router).await?;
        Ok(())
    }
}

async fn upgrade(ws: WebSocketUpgrade, Query(q): Query<After>, State(s): State<Server>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client(socket, s, q.after))
}

async fn client(mut socket: WebSocket, s: Server, after: u64) {
    // 購読してから再送ぶんを取り出す（間に来た出来事を落とさない）
    let mut rx = s.inner.tx.subscribe();
    let (first, sent_upto) = {
        let log = s.inner.log.lock().unwrap();
        let hello = json!({"type": "hello", "app": "splat-result-watcher",
            "version": env!("CARGO_PKG_VERSION"), "last_seq": log.last_seq})
        .to_string();
        let mut first = vec![hello];
        first.extend(log.events.iter().filter(|(q, _)| *q > after).map(|(_, t)| t.clone()));
        first.push(status_json(log.stage));
        (first, log.last_seq)
    };
    for text in first {
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    let mut beat = tokio::time::interval(BEAT);
    beat.tick().await;
    loop {
        tokio::select! {
            msg = rx.recv() => {
                let text = match msg {
                    Ok(t) => t,
                    // 遅れて取りこぼした。切ればクライアントが after を付けてつなぎ直す
                    Err(_) => return,
                };
                // 再送ぶんに含めたものは二度送らない
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if v["seq"].as_u64().is_some_and(|q| q <= sent_upto) {
                        continue;
                    }
                }
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
            _ = beat.tick() => {
                let stage = s.inner.log.lock().unwrap().stage;
                if socket.send(Message::Text(status_json(stage).into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    async fn next(ws: &mut (impl StreamExt<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin)) -> Value {
        let msg = tokio::time::timeout(Duration::from_secs(3), ws.next()).await.unwrap().unwrap().unwrap();
        serde_json::from_str(msg.to_text().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn hello_backlog_status_then_live() {
        let dir = std::env::temp_dir().join(format!("srw-test-{}", std::process::id()));
        let path = dir.join("events.jsonl");
        let _ = std::fs::remove_file(&path);

        let s = Server::open(&path).unwrap();
        for i in 0..3 {
            s.publish(json!({"type": "result", "match_id": format!("m{i}")})).unwrap();
        }
        s.set_stage("post_match");
        // 開き直しても seq は続きから
        let s = Server::open(&path).unwrap();
        assert_eq!(s.last_seq(), 3);
        s.set_stage("post_match");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(s.clone().serve_on(listener));

        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/events?after=1"))
            .await
            .unwrap();
        let hello = next(&mut ws).await;
        assert_eq!((hello["type"].as_str(), hello["last_seq"].as_u64()), (Some("hello"), Some(3)));
        assert_eq!(next(&mut ws).await["seq"], 2);
        assert_eq!(next(&mut ws).await["seq"], 3);
        let st = next(&mut ws).await;
        assert_eq!((st["type"].as_str(), st["state"].as_str()), (Some("status"), Some("post_match")));

        s.set_stage("in_battle");
        assert_eq!(next(&mut ws).await["state"], "in_battle");
        let seq = s.publish(json!({"type": "battle_started", "match_id": "m3"})).unwrap();
        let ev = next(&mut ws).await;
        assert_eq!((ev["seq"].as_u64(), ev["type"].as_str()), (Some(seq), Some("battle_started")));
        assert_eq!(seq, 4);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
