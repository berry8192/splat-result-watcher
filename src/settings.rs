//! 利用者が変えられる設定。`%LOCALAPPDATA%\splat-result-watcher\settings.json`。
//! 待ち受けの番号と撮る幅は、起動し直したときに効く。録画の入り切りはすぐ効く。

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::data_dir;
use crate::engine::EngineConfig;
use crate::source::{CaptureConfig, CaptureFrom};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// WebSocket の待ち受けの番号（127.0.0.1 のみ）
    pub port: u16,
    /// N Air の出力を撮る幅（1280 ならゲーム穴が照合の大きさにちょうどなる）
    pub width: u32,
    /// 見本の録画を回す
    pub record: bool,
    /// 見本の録画の上限（GB）。超えたら古い順に消す
    pub record_cap_gb: f64,
    /// デバッグ用に、何かに当たったフレームの読みと画面を残す（hits\日付\。上限 500MB）
    pub hit_log: bool,
    /// どの配信ソフトから撮るか（auto: N Air が起きていればそれ、無ければ OBS）
    pub capture_from: CaptureFrom,
    /// OBS の obs-websocket の番号とパスワード（OBS の「ツール → WebSocket サーバー設定」）
    pub obs_port: u16,
    pub obs_password: String,
    /// 見せる窓の見た目（見せる窓の右クリックと、設定の窓から変える。すぐ効く）
    pub display: DisplaySettings,
}

/// 見せる窓の見た目。字の大きさは論理 px。窓の大きさは中身に合わせて決まる
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    /// "yoko"（横長: 左にモードとルール・勝敗、右にパワー）か "tate"（縦長: 上から順）
    pub layout: String,
    /// CSS の色。"transparent" なら窓を透かす（配信ソフトのウィンドウキャプチャで透過を許可する）
    pub bg: String,
    pub font_head: u32,
    pub font_power: u32,
    pub font_set: u32,
    /// 字の縁取りの太さ（px。0 で無し）と色。透明な背景で読めるようにする
    pub outline_px: u32,
    pub outline_color: String,
    /// モード名の色（「Xマッチ」は青緑、「バンカラ」は橙。ゲームの配色に合わせた既定）
    pub x_color: String,
    pub bankara_color: String,
    /// パワー（既定は白）・勝敗（薄い黄）・ルール名（青）の色
    pub power_color: String,
    pub set_color: String,
    pub head_color: String,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        DisplaySettings { layout: "yoko".into(), bg: "#16161d".into(), font_head: 22, font_power: 72, font_set: 30, outline_px: 0, outline_color: "#000000".into(), x_color: "#2bd9c4".into(), bankara_color: "#ff7a2e".into(), power_color: "#ffffff".into(), set_color: "#f3ea6a".into(), head_color: "#8fc5ff".into() }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            port: 3140,
            width: 1280,
            record: false,
            record_cap_gb: 20.0,
            hit_log: true,
            capture_from: CaptureFrom::Auto,
            obs_port: crate::obs::DEFAULT_PORT,
            obs_password: String::new(),
            display: DisplaySettings::default(),
        }
    }
}

impl Settings {
    pub fn capture_config(&self) -> CaptureConfig {
        CaptureConfig { from: self.capture_from, obs_port: self.obs_port, obs_password: self.obs_password.clone() }
    }

    pub fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    /// 読む。無い・壊れているときは既定（壊れていたら記録に残せるよう理由も返す）
    pub fn load() -> (Self, Option<String>) {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => (s, None),
                Err(e) => (Self::default(), Some(format!("{} を読めないので既定にした: {e}", path.display()))),
            },
            Err(_) => (Self::default(), None),
        }
    }

    pub fn check(&self) -> Result<()> {
        if self.port < 1024 {
            bail!("待ち受けの番号は 1024 以上にする");
        }
        if !(640..=1920).contains(&self.width) {
            bail!("撮る幅は 640〜1920 にする（1280 を勧める）");
        }
        if self.record_cap_gb.is_nan() || self.record_cap_gb <= 0.0 {
            bail!("録画の上限は 0 より大きくする");
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.check()?;
        let path = Self::path();
        std::fs::create_dir_all(data_dir())?;
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text).with_context(|| format!("{} に書けない", path.display()))
    }

    pub fn engine_config(&self) -> EngineConfig {
        EngineConfig {
            addr: SocketAddr::from(([127, 0, 0, 1], self.port)),
            width: self.width,
            record: self.record,
            record_cap_bytes: (self.record_cap_gb * 1024.0 * 1024.0 * 1024.0) as u64,
            hits_dir: self.hit_log.then(|| data_dir().join("hits")),
            capture: self.capture_config(),
            ..EngineConfig::default()
        }
    }
}
