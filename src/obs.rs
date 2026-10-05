//! OBS Studio の出力を撮る。obs-websocket（v5。OBS 28 から同梱）に WebSocket でつなぎ、
//! `GetSourceScreenshot` で**合成済みのプログラム出力**をもらう（N Air と違って窓を作る必要が無い）。
//!
//! 利用者の準備: OBS の「ツール → WebSocket サーバー設定」で有効にし、パスワードを設定に入れる（既定の番号は 4455）。
//! 撮る大きさは `width`×(16:9)。OBS はキャンバスをその大きさに縮めて返すので、キャンバスが 16:9 でなければ縦横比が崩れる。

use std::net::TcpStream;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use image::RgbImage;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

pub const DEFAULT_PORT: u16 = 4455;

pub struct Obs {
    sock: WebSocket<MaybeTlsStream<TcpStream>>,
    size: (u32, u32),
    next_id: u64,
    broken: bool,
}

impl Obs {
    /// つないで名乗る（パスワードが設定されていれば認証する）
    pub fn connect(port: u16, password: &str, width: u32) -> Result<Self> {
        let (mut sock, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}"))
            .with_context(|| format!("OBS の WebSocket（127.0.0.1:{port}）につなげない。OBS の「ツール → WebSocket サーバー設定」で有効にする"))?;
        if let MaybeTlsStream::Plain(s) = sock.get_ref() {
            s.set_read_timeout(Some(Duration::from_secs(3)))?;
            s.set_write_timeout(Some(Duration::from_secs(3)))?;
        }
        let hello = read_json(&mut sock)?;
        if hello["op"] != 0 {
            bail!("OBS の最初の応答が Hello でない: {hello}");
        }
        let mut identify = json!({"rpcVersion": 1, "eventSubscriptions": 0});
        if let Some(auth) = hello["d"]["authentication"].as_object() {
            if password.is_empty() {
                bail!("OBS の WebSocket にパスワードが要る。設定に入れる");
            }
            let salt = auth["salt"].as_str().unwrap_or_default();
            let challenge = auth["challenge"].as_str().unwrap_or_default();
            let b64 = base64::engine::general_purpose::STANDARD;
            let secret = b64.encode(Sha256::new().chain_update(password).chain_update(salt).finalize());
            let answer = b64.encode(Sha256::new().chain_update(&secret).chain_update(challenge).finalize());
            identify["authentication"] = answer.into();
        }
        sock.send(Message::text(json!({"op": 1, "d": identify}).to_string()))?;
        let identified = read_json(&mut sock)?;
        if identified["op"] != 2 {
            bail!("OBS に名乗れなかった（パスワード違いか）: {identified}");
        }
        Ok(Obs { sock, size: (width, width * 9 / 16), next_id: 0, broken: false })
    }

    /// 撮れなくなっていないか（撮影に失敗したら開き直す）
    pub fn alive(&self) -> bool {
        !self.broken
    }

    fn request(&mut self, kind: &str, data: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let req = json!({"op": 6, "d": {"requestType": kind, "requestId": id, "requestData": data}});
        let r = (|| -> Result<Value> {
            self.sock.send(Message::text(req.to_string()))?;
            // 出来事（op 5）は申し込んでいないが、念のため自分の応答が来るまで読む
            for _ in 0..20 {
                let v = read_json(&mut self.sock)?;
                if v["op"] == 7 && v["d"]["requestId"] == id {
                    let st = &v["d"]["requestStatus"];
                    if st["result"] != true {
                        bail!("OBS が {kind} を断った: {}", st["comment"].as_str().unwrap_or("理由なし"));
                    }
                    return Ok(v["d"]["responseData"].clone());
                }
            }
            bail!("OBS から {kind} の応答が来ない")
        })();
        if r.is_err() {
            self.broken = true;
        }
        r
    }

    /// プログラム出力を 1 枚撮る（RGB、`width`×(16:9)）
    pub fn capture(&mut self) -> Result<RgbImage> {
        let scene = self.request("GetCurrentProgramScene", json!({}))?;
        let name = scene["sceneName"]
            .as_str()
            .or(scene["currentProgramSceneName"].as_str())
            .ok_or_else(|| anyhow!("OBS のシーン名が取れない"))?
            .to_string();
        let (w, h) = self.size;
        let shot = self.request(
            "GetSourceScreenshot",
            json!({"sourceName": name, "imageFormat": "png", "imageWidth": w, "imageHeight": h}),
        )?;
        let data = shot["imageData"].as_str().context("OBS の画像が無い")?;
        let b64 = data.rsplit_once(',').map_or(data, |(_, b)| b);
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).context("OBS の画像が読めない")?;
        let img = image::load_from_memory(&bytes).context("OBS の画像が解けない")?.to_rgb8();
        if (img.width(), img.height()) != (w, h) {
            bail!("OBS の画像が {}×{}（{w}×{h} のはず）", img.width(), img.height());
        }
        Ok(img)
    }
}

fn read_json(sock: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<Value> {
    loop {
        match sock.read().context("OBS との接続が切れた")? {
            Message::Text(t) => return Ok(serde_json::from_str(t.as_str())?),
            Message::Binary(b) => return Ok(serde_json::from_slice(&b)?),
            Message::Close(_) => bail!("OBS が接続を閉じた"),
            _ => {}
        }
    }
}
