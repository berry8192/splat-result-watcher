//! 利用者が変えられる設定。`%LOCALAPPDATA%\splat-result-watcher\settings.json`。
//! 待ち受けの番号と撮る幅は、起動し直したときに効く。録画の入り切りはすぐ効く。

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::data_dir;
use crate::engine::EngineConfig;

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
}

impl Default for Settings {
    fn default() -> Self {
        Settings { port: 3140, width: 1280, record: false, record_cap_gb: 20.0 }
    }
}

impl Settings {
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
            ..EngineConfig::default()
        }
    }
}
