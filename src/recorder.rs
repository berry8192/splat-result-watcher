//! 見本集めの録画。撮った絵を縮小して JPEG で残し、合計が上限を超えたら古い順に消す。
//! 配信中ずっと ON にしておけるよう、書き出しは撮影と別のスレッドで行う。
//!
//! 置き場所: `<root>/<開始日時>/<時刻>.jpg`（時刻はローカル、ミリ秒まで）。
//! ファイル名の並びがそのまま時刻の並びになるので、古い順はファイル名で決まる。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::RgbImage;

pub struct RecorderConfig {
    pub root: PathBuf,
    /// 縮小後の幅（高さは比を保つ）。元より大きければ縮小しない
    pub width: u32,
    pub quality: u8,
    /// 全セッション合計の上限
    pub cap_bytes: u64,
}

pub struct Recorder {
    tx: Option<SyncSender<(DateTime<Local>, RgbImage)>>,
    worker: Option<JoinHandle<()>>,
    pub session_dir: PathBuf,
}

impl Recorder {
    pub fn start(cfg: RecorderConfig) -> Result<Self> {
        let session_dir = cfg
            .root
            .join(Local::now().format("%Y%m%d-%H%M%S").to_string());
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("{} を作れない", session_dir.display()))?;
        let files = existing_files(&cfg.root);

        // 書き出しが追いつかないときは撮影を待たせずに捨てる（溜めすぎるとメモリを食う）
        let (tx, rx) = mpsc::sync_channel(4);
        let dir = session_dir.clone();
        let worker = std::thread::spawn(move || write_loop(rx, dir, cfg, files));
        Ok(Recorder {
            tx: Some(tx),
            worker: Some(worker),
            session_dir,
        })
    }

    /// 1 枚渡す。書き出しが詰まっていれば捨てて false を返す
    pub fn push(&self, at: DateTime<Local>, img: RgbImage) -> bool {
        match self.tx.as_ref().map(|tx| tx.try_send((at, img))) {
            Some(Ok(())) => true,
            Some(Err(TrySendError::Full(_))) | Some(Err(TrySendError::Disconnected(_))) | None => {
                false
            }
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // 送り口を閉じると書き出し側が残りを書いて抜ける
        self.tx.take();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// 既にある見本を古い順に（パスと大きさ）
fn existing_files(root: &Path) -> VecDeque<(PathBuf, u64)> {
    let mut all = Vec::new();
    let Ok(sessions) = std::fs::read_dir(root) else {
        return VecDeque::new();
    };
    for s in sessions.flatten() {
        let Ok(entries) = std::fs::read_dir(s.path()) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "jpg") {
                let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                all.push((p, len));
            }
        }
    }
    // フォルダ名もファイル名も日時なので、パスの並びが時刻の並び
    all.sort();
    all.into()
}

fn write_loop(
    rx: Receiver<(DateTime<Local>, RgbImage)>,
    dir: PathBuf,
    cfg: RecorderConfig,
    mut files: VecDeque<(PathBuf, u64)>,
) {
    let mut total: u64 = files.iter().map(|(_, n)| n).sum();
    for (at, img) in rx {
        let img = if img.width() > cfg.width {
            let h = (img.height() as u64 * cfg.width as u64 / img.width() as u64) as u32;
            image::imageops::resize(&img, cfg.width, h, FilterType::Triangle)
        } else {
            img
        };
        let path = dir.join(format!("{}.jpg", at.format("%H%M%S_%3f")));
        let mut bytes = Vec::new();
        if let Err(e) = JpegEncoder::new_with_quality(&mut bytes, cfg.quality).encode_image(&img) {
            eprintln!("JPEG にできない: {}", e);
            continue;
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            eprintln!("{} を書けない: {}", path.display(), e);
            continue;
        }
        total += bytes.len() as u64;
        files.push_back((path, bytes.len() as u64));

        while total > cfg.cap_bytes {
            let Some((old, n)) = files.pop_front() else {
                break;
            };
            let _ = std::fs::remove_file(&old);
            total -= n;
            // 空になったセッションのフォルダも片付ける（今のフォルダは残す）
            if let Some(parent) = old.parent() {
                if parent != dir {
                    let _ = std::fs::remove_dir(parent);
                }
            }
        }
    }
}
