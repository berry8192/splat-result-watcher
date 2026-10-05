//! デバッグ用の「当たり」の記録。後から人や Claude が読み返して、何をどう見分けたかを確かめる。
//!
//! 置き場所: `%LOCALAPPDATA%\splat-result-watcher\hits\YYYYMMDD\`
//! - `hits.jsonl`: 1 行 1 件の JSON。何か（見本・形と色・数字・自動で足した見本・流した出来事）に当たったフレームのうち、
//!   中身が前の記録と変わったものだけ。項目は [`Record`]
//! - `HHMMSS_mmm.jpg`: そのときのゲーム穴（照合する大きさ。`image` の項目にファイル名）
//!
//! 全部で `CAP_BYTES` を超えたら、古い日から消す。

use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::Local;
use image::RgbImage;
use serde::Serialize;
use serde_json::Value;

use crate::recognize::Reading;

/// 記録の上限（全部の日の合計）
const CAP_BYTES: u64 = 500 * 1024 * 1024;
/// 何件ごとに大きさを確かめるか
const CHECK_EVERY: u32 = 100;

#[derive(Serialize)]
struct Number<'a> {
    place: &'a str,
    /// 見本で読めた字（読めない字は `?`）
    text: &'a str,
    /// 読めない字を手がかりの数字で推測したもの
    guess: &'a str,
    /// 1 文字ずつ: [字, 一致度]
    chars: &'a [(char, f64)],
}

#[derive(Serialize)]
struct Shape<'a> {
    place: &'a str,
    label: &'a str,
}

/// `hits.jsonl` の 1 行
#[derive(Serialize)]
struct Record<'a> {
    /// 端末の時刻
    at: String,
    /// 試合の段階（idle / in_battle / reading / post_match / no_signal）
    stage: &'a str,
    /// このフレームで見えたもの（state.rs の `Seen`）
    seen: String,
    /// 見本ではなく形と色で見分けた見出し
    shapes: Vec<Shape<'a>>,
    /// 読んだ数字
    numbers: Vec<Number<'a>>,
    /// 場所ごとの一番近い見本と一致度: [場所, ラベルか読み, 一致度]
    peaks: &'a [(String, String, f64)],
    /// 途中経過（GUI に出すものと同じ）
    notes: &'a [String],
    /// このフレームで自動で足した見本
    learned: &'a [String],
    /// このフレームで流した出来事
    events: &'a [Value],
    /// 画面の JPEG のファイル名（同じフォルダ）
    image: Option<String>,
}

pub struct HitLog {
    root: PathBuf,
    last_key: String,
    count: u32,
}

impl HitLog {
    pub fn new(root: PathBuf) -> Self {
        HitLog { root, last_key: String::new(), count: 0 }
    }

    /// 何かに当たっていて、前の記録と中身が違えば残す。残したら JSON の行を返す（記録に出すため）
    pub fn record(&mut self, stage: &str, r: &Reading, learned: &[String], events: &[Value], game: Option<&RgbImage>) -> Option<String> {
        use crate::state::Seen;
        let hit = !matches!(r.seen, Seen::Unknown | Seen::NoSignal)
            || !r.shape_labels.is_empty()
            || !r.numbers.is_empty()
            || !learned.is_empty()
            || !events.is_empty();
        if !hit {
            self.last_key.clear();
            return None;
        }
        let seen = format!("{:?}", r.seen);
        let shapes: Vec<Shape> = r.shape_labels.iter().map(|s| Shape { place: s.place, label: s.label }).collect();
        let numbers: Vec<Number> = r
            .numbers
            .iter()
            .map(|(place, g)| Number { place, text: &g.text, guess: &g.guess, chars: &g.chars })
            .collect();
        let key = format!(
            "{seen}|{}|{}",
            shapes.iter().map(|s| format!("{}={}", s.place, s.label)).collect::<Vec<_>>().join(","),
            numbers.iter().map(|n| format!("{}={}/{}", n.place, n.text, n.guess)).collect::<Vec<_>>().join(",")
        );
        if key == self.last_key && learned.is_empty() && events.is_empty() {
            return None;
        }
        self.last_key = key;

        let now = Local::now();
        let dir = self.root.join(now.format("%Y%m%d").to_string());
        if std::fs::create_dir_all(&dir).is_err() {
            return None;
        }
        let image = game.and_then(|g| {
            let name = format!("{}.jpg", now.format("%H%M%S_%3f"));
            let mut buf = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85).encode_image(g).ok()?;
            std::fs::write(dir.join(&name), buf).ok()?;
            Some(name)
        });
        let rec = Record {
            at: now.to_rfc3339(),
            stage,
            seen,
            shapes,
            numbers,
            peaks: &r.peaks,
            notes: &r.notes,
            learned,
            events,
            image,
        };
        let line = serde_json::to_string(&rec).ok()?;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("hits.jsonl")).ok()?;
        writeln!(f, "{line}").ok()?;

        self.count += 1;
        if self.count % CHECK_EVERY == 1 {
            prune(&self.root, CAP_BYTES);
        }
        Some(line)
    }
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

/// 合計が `cap` を超えていたら、古い日のフォルダから消す（今日の分は残す）
fn prune(root: &Path, cap: u64) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut days: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect();
    days.sort();
    let mut total: u64 = days.iter().map(|d| dir_size(d)).sum();
    for d in days.iter().take(days.len().saturating_sub(1)) {
        if total <= cap {
            break;
        }
        total -= dir_size(d);
        let _ = std::fs::remove_dir_all(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Outcome, Seen};

    fn reading(seen: Seen) -> Reading {
        Reading { seen, ..Reading::no_signal() }
    }

    #[test]
    fn only_hits_that_change_are_kept() {
        let root = std::env::temp_dir().join(format!("srw-hits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut h = HitLog::new(root.clone());
        let img = RgbImage::new(16, 9);
        assert!(h.record("idle", &reading(Seen::Unknown), &[], &[], Some(&img)).is_none());
        assert!(h.record("reading", &reading(Seen::Outcome(Outcome::Win)), &[], &[], Some(&img)).is_some());
        // 同じものが続く間は残さない
        assert!(h.record("reading", &reading(Seen::Outcome(Outcome::Win)), &[], &[], Some(&img)).is_none());
        // 出来事を流したフレームは残す
        let ev = vec![serde_json::json!({"type": "result"})];
        let line = h.record("reading", &reading(Seen::Outcome(Outcome::Win)), &[], &ev, Some(&img)).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["seen"], "Outcome(Win)");
        assert_eq!(v["events"][0]["type"], "result");
        let day = std::fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
        let jsonl = std::fs::read_to_string(day.join("hits.jsonl")).unwrap();
        assert_eq!(jsonl.lines().count(), 2);
        let image = v["image"].as_str().unwrap();
        assert!(day.join(image).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
