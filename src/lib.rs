//! splat-result-watcher の中身。コマンドの exe（`src/main.rs`）と GUI の exe（`src/bin/gui.rs`）が使う。

pub mod engine;
pub mod layout;
pub mod learn;
pub mod matching;
pub mod nair;
pub mod recognize;
pub mod recorder;
pub mod server;
pub mod settings;
pub mod shapes;
pub mod starter;
pub mod state;
pub mod templates;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

/// 撮る間隔。1 回の撮影が重ければ（平均 20ms 超）1 秒に落とす
pub const INTERVAL: Duration = Duration::from_millis(500);
pub const SLOW_INTERVAL: Duration = Duration::from_secs(1);
pub const SLOW_CAPTURE_MS: f64 = 20.0;
/// これより黒い絵は「映像が来ていない」とみなす
pub const NO_SIGNAL_DARK: f64 = 0.98;

/// 見本の置き場所。repo の `samples/`（git 管理外。ゲーム画面の切り抜きは同梱しない）。
/// exe をどこから起動しても同じ所に貯まるよう、ビルドしたときの repo の場所を使う
pub fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples")
}

/// 利用者ごとのファイル（出来事の控え・登録した見本・閉じ損ねたプロジェクターの控え）の置き場所
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("splat-result-watcher")
}

pub fn open_projector(width: u32) -> Result<nair::Projector> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    nair::Projector::open(Some(&dir.join("projector.hwnd")), width)
}
