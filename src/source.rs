//! 配信の出力を撮る口。N Air（`nair.rs`、プロジェクターの窓を撮る）と OBS Studio（`obs.rs`、obs-websocket）の
//! どちらかを開き、以降は同じ扱いで撮る。

use anyhow::{bail, Result};
use image::RgbImage;
use serde::{Deserialize, Serialize};

use crate::nair::Projector;
use crate::obs::Obs;

/// どの配信ソフトから撮るか
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureFrom {
    /// N Air が起きていればそれ、無ければ OBS
    #[default]
    Auto,
    NAir,
    Obs,
}

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub from: CaptureFrom,
    pub obs_port: u16,
    pub obs_password: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        CaptureConfig { from: CaptureFrom::Auto, obs_port: crate::obs::DEFAULT_PORT, obs_password: String::new() }
    }
}

pub enum Source {
    NAir(Projector),
    Obs(Obs),
}

impl Source {
    /// 設定に従って開く。auto は N Air を先に試す（N Air が無いときの失敗はすぐ返る）
    pub fn open(width: u32, cfg: &CaptureConfig) -> Result<Self> {
        let nair = || crate::open_projector(width).map(Source::NAir);
        let obs = || Obs::connect(cfg.obs_port, &cfg.obs_password, width).map(Source::Obs);
        match cfg.from {
            CaptureFrom::NAir => nair(),
            CaptureFrom::Obs => obs(),
            CaptureFrom::Auto => match nair() {
                Ok(s) => Ok(s),
                Err(e1) => match obs() {
                    Ok(s) => Ok(s),
                    Err(e2) => bail!("N Air: {e1:#} / OBS: {e2:#}"),
                },
            },
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Source::NAir(_) => "N Air",
            Source::Obs(_) => "OBS",
        }
    }

    pub fn alive(&self) -> bool {
        match self {
            Source::NAir(p) => p.alive(),
            Source::Obs(o) => o.alive(),
        }
    }

    pub fn capture(&mut self) -> Result<RgbImage> {
        match self {
            Source::NAir(p) => p.capture(),
            Source::Obs(o) => o.capture(),
        }
    }
}
