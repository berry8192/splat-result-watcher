//! 見本を自動で足す。読めない字（`?`）が混じった数字を、ほかの確かな数字から計算や同じ値で埋め、
//! 確度の高いものだけ、その字の見本として登録する（利用者が数字を入れる手間を減らす）。
//!
//! 埋め方:
//! - 同じ画面の中の計算: ウデマエ「動く前 + TOTAL = 動いた後」、X パワー「動く前 + 増減 = 動いた後」
//! - 同じ値: 試合後の値（X パワー・ウデマエ・昇格の 300p）と、その後ロビーのメニューに出る値
//!   （間にルール紹介が無く、5 分以内。X は同じルールの時間帯のうち）
//!
//! 確度の条件（どれか外れたら足さない）:
//! - どちらの数字も 3 回続けて同じ読み（動いている途中の数字を使わない）
//! - 答えの側は読めない字が無い
//! - 埋める側の読めない字は 1〜2 字で、読めた数字は 2 字以上。読めた字・小数点・マイナスの位置が答えと全部合う
//! - 埋める字が、ほかの字の見本（どの種類の数字でも）に `CONFLICT` 以上似ていない
//! - 自動で足す見本は、字ごとに `MAX_AUTO` 個まで

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use crate::matching::{self, Glyph, Patch};
use crate::recognize::{parse_delta, parse_points, parse_power, GlyphRead};
use crate::state::Seen;
use crate::templates::{self, glyph_char, glyph_label, is_auto, Pool, Templates};

/// 何回続けて同じ読みなら落ち着いたとみなすか
const STABLE: u32 = 3;
/// 続けて読んだとみなす間（これより空いたら数え直す）
const RUN_GAP: i64 = 3;
/// 同じ画面とみなす間（これより空いたら、その場所の落ち着いた値の並びを捨てる）
const EPISODE_GAP: i64 = 15;
/// 試合後の値とメニューの値を同じとみなす間
const SAME_VALUE_WITHIN: i64 = 5 * 60;
const MAX_UNKNOWN: usize = 2;
const MIN_KNOWN: usize = 2;
/// ほかの字の見本にこれ以上似ていたら、埋めた答えを疑って足さない
/// （同じ字体の違う字どうしは 0.79 以下、字体をまたいだ同じ字は 0.80〜0.95 だった）
const CONFLICT: f64 = 0.80;
const MAX_AUTO: usize = 3;

const DIGIT_POOLS: [Pool; 5] = [Pool::Digit, Pool::DigitSmall, Pool::DigitGauge, Pool::DigitTotal, Pool::DigitMenu];

/// 試合後の値と、同じ値が出るメニューの場所（X はルールの時間帯も合わせる）
const SAME_VALUE: [(&str, &str, bool); 3] = [
    ("power_number", "menu_x_value", true),
    ("udemae_value", "menu_udemae_value", false),
    ("udemae_reset", "menu_udemae_value", false),
];

/// 自動で足す見本 1 つ
#[derive(Clone, Debug)]
pub struct Learned {
    pub pool: Pool,
    pub label: String,
    pub patch: Patch,
    /// 記録に残す理由
    pub why: String,
}

#[derive(Clone, Debug)]
struct Settled {
    at: DateTime<Utc>,
    read: GlyphRead,
    /// 何回目のルール紹介の後か
    battle: u64,
}

#[derive(Default)]
struct Track {
    text: String,
    run: u32,
    last: Option<DateTime<Utc>>,
    /// この画面で落ち着いた値（古い順、続けて同じものは 1 つ）
    settled: Vec<Settled>,
}

#[derive(Default)]
pub struct Learner {
    tracks: HashMap<&'static str, Track>,
    battle: u64,
    in_intro: bool,
}

/// 「2194.6」「+94.6」を 0.1 単位の整数に
fn tenths(v: f64) -> i64 {
    (v * 10.0).round() as i64
}

fn power_text(t: i64) -> String {
    format!("{}.{}", t / 10, t % 10)
}

fn delta_text(t: i64) -> String {
    format!("{}{}.{}", if t < 0 { '-' } else { '+' }, t.abs() / 10, t.abs() % 10)
}

fn complete(r: &GlyphRead) -> bool {
    !r.text.contains('?')
}

/// ルールの時間帯（2 時間ごと、奇数時に変わる。state.rs と同じ）
fn slot(at: DateTime<Utc>) -> i64 {
    (at.timestamp() - 3600).div_euclid(7200)
}

impl Learner {
    /// 1 フレームぶん。足してよい見本を返す（足すのは呼ぶ側）
    pub fn feed(&mut self, at: DateTime<Utc>, seen: &Seen, numbers: &[(&'static str, GlyphRead)], t: &Templates) -> Vec<Learned> {
        let intro = matches!(seen, Seen::RuleIntro(_));
        if intro && !self.in_intro {
            self.battle += 1;
        }
        self.in_intro = intro;

        let mut out: Vec<Learned> = Vec::new();
        for (place, read) in numbers {
            let battle = self.battle;
            let tr = self.tracks.entry(place).or_default();
            let gap = tr.last.map_or(i64::MAX, |l| (at - l).num_seconds());
            if gap > EPISODE_GAP {
                tr.settled.clear();
            }
            // 推測まで同じなら同じ絵（推測は切り出した形だけで決まる）
            if gap > RUN_GAP || tr.text != read.guess {
                tr.text = read.guess.clone();
                tr.run = 0;
            }
            tr.run += 1;
            tr.last = Some(at);
            if tr.run != STABLE || read.chars.is_empty() {
                continue;
            }
            if tr.settled.last().is_none_or(|s| s.read.guess != read.guess) {
                tr.settled.push(Settled { at, read: read.clone(), battle });
            }
            for l in self.infer(place, t) {
                if !out.iter().any(|o| o.pool == l.pool && o.patch.px == l.patch.px) {
                    out.push(l);
                }
            }
        }
        out
    }

    fn settled(&self, place: &str) -> &[Settled] {
        self.tracks.get(place).map_or(&[], |t| t.settled.as_slice())
    }

    /// `place` が落ち着いたときに、埋められるものを探す
    fn infer(&self, place: &str, t: &Templates) -> Vec<Learned> {
        let mut out = Vec::new();

        // 同じ画面の中の計算: 動く前 + 増減（TOTAL）= 動いた後
        let points = |s: &str| parse_points(s).map(i64::from);
        let power = |s: &str| parse_power(s).map(tenths);
        let delta = |s: &str| parse_delta(s).map(tenths);
        type Parse<'a> = &'a dyn Fn(&str) -> Option<i64>;
        type Show = fn(i64) -> String;
        let sums: [(&str, &str, &str, Parse, Parse, Show, Show); 2] = [
            ("udemae_value", "udemae_total", "ウデマエ", &points, &points, |v| v.to_string(), |v| v.to_string()),
            ("power_number", "power_delta", "X パワー", &power, &delta, power_text, delta_text),
        ];
        for (value_id, delta_id, name, parse_v, parse_d, show_v, show_d) in sums {
            if place != value_id && place != delta_id {
                continue;
            }
            let s = self.settled(value_id);
            let (Some(b), Some(a), Some(d)) = (s.first(), s.last(), self.settled(delta_id).last()) else { continue };
            if s.len() < 2 {
                continue;
            }
            let (b, a, d) = (&b.read, &a.read, &d.read);
            let why = format!("{name} {} と {} で {} になった", b.guess, d.guess, a.guess);
            // 確かな読み（見本で読めた）と、推測込みの読み
            let (cb, ca, cd) = (parse_v(&b.text), parse_v(&a.text), parse_d(&d.text));
            let (gb, ga, gd) = (cb.or(parse_v(&b.guess)), ca.or(parse_v(&a.guess)), cd.or(parse_d(&d.guess)));

            // 推測込みで 3 つとも読めて計算がぴったり合えば、推測した字を見本にする
            // （別々に読んだ 3 つの数字が合うので、推測の読み違いはまず残らない）
            if let (Some(vb), Some(va), Some(vd)) = (gb, ga, gd) {
                if vb + vd == va && va != vb {
                    for (id, r, certain) in [(value_id, b, cb), (value_id, a, ca), (delta_id, d, cd)] {
                        if certain.is_none() {
                            out.extend(fill(id, r, &r.guess, &format!("{why}（推測が計算で確かめられた）"), Verified::Sum, t));
                        }
                    }
                    continue;
                }
            }
            // 確かな読みが 2 つあれば、残りの 1 つを計算で埋める
            let why = |x: &str| format!("{why}（{x}を計算で埋めた）");
            match (cb, ca, cd) {
                (None, Some(va), Some(vd)) => out.extend(fill(value_id, b, &show_v(va - vd), &why("動く前"), Verified::Few, t)),
                (Some(vb), None, Some(vd)) => out.extend(fill(value_id, a, &show_v(vb + vd), &why("動いた後"), Verified::Few, t)),
                (Some(vb), Some(va), None) if va != vb => out.extend(fill(delta_id, d, &show_d(va - vb), &why("増減"), Verified::Few, t)),
                _ => {}
            }
        }

        // 試合後の値 = その後のメニューの値
        for (result, menu, by_slot) in SAME_VALUE {
            if place != result && place != menu {
                continue;
            }
            let (Some(r), Some(m)) = (self.settled(result).last(), self.settled(menu).last()) else { continue };
            let close = r.battle == m.battle
                && r.at <= m.at
                && (m.at - r.at) <= Duration::seconds(SAME_VALUE_WITHIN)
                && (!by_slot || slot(r.at) == slot(m.at));
            if !close {
                continue;
            }
            let why = |x: &str, v: &str| format!("試合後の {result} と、その後のメニューの {menu} は同じ値 {v}（{x}を埋めた）");
            match (complete(&r.read), complete(&m.read)) {
                (true, false) => out.extend(fill(menu, &m.read, &r.read.text, &why("メニュー", &r.read.text), Verified::Few, t)),
                (false, true) => out.extend(fill(result, &r.read, &m.read.text, &why("試合後", &m.read.text), Verified::Few, t)),
                _ => {}
            }
        }
        out
    }
}

/// 埋める答えの確かさ
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verified {
    /// 推測込みの 3 つの数字の計算が合った（読めない字がいくつあってもよい）
    Sum,
    /// 確かな読みから出した答え（読めない字は少しだけ、読めた字が答えと合うこと）
    Few,
}

/// `read` の読めない字を `expected` で埋める。確度の条件を満たさなければ空
fn fill(place_id: &str, read: &GlyphRead, expected: &str, why: &str, how: Verified, t: &Templates) -> Vec<Learned> {
    let Some(place) = templates::place(place_id) else { return Vec::new() };
    let got: Vec<char> = read.text.chars().collect();
    let want: Vec<char> = expected.chars().collect();
    if got.len() != want.len() || read.glyphs.len() != got.len() {
        return Vec::new();
    }
    let unknown: Vec<usize> = (0..got.len()).filter(|&i| got[i] == '?').collect();
    let known = got.iter().filter(|c| c.is_ascii_digit()).count();
    if unknown.is_empty() || (how == Verified::Few && (unknown.len() > MAX_UNKNOWN || known < MIN_KNOWN)) {
        return Vec::new();
    }
    // 手がかりの数字の推測が答えと食い違う字があれば、答えか切り出しを疑う
    let guessed: Vec<char> = read.guess.chars().collect();
    if guessed.len() == want.len() && unknown.iter().any(|&i| guessed[i] != '?' && guessed[i] != want[i]) {
        return Vec::new();
    }
    if (0..got.len()).any(|i| got[i] != '?' && got[i] != want[i]) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in unknown {
        let (Some(label), Glyph::Shape(patch)) = (glyph_label(want[i]), &read.glyphs[i]) else {
            return Vec::new();
        };
        // ほかの字の見本によく似ていたら、答えか切り出しを疑う（1 字でも当たれば全部やめる）
        let conflict = DIGIT_POOLS.iter().flat_map(|&p| t.get(p)).any(|tm| {
            glyph_char(&tm.label) != Some(want[i]) && matching::glyph_iou(patch, &tm.patch) >= CONFLICT
        });
        if conflict {
            return Vec::new();
        }
        let autos = t.get(place.pool).iter().filter(|tm| tm.label == label && is_auto(&tm.id)).count();
        if autos >= MAX_AUTO {
            continue;
        }
        out.push(Learned { pool: place.pool, label, patch: patch.clone(), why: format!("{} → {expected}。{why}", read.text) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Rule;

    /// 字ごとに違う形（30×30 の中の、字の番号の位置の横線）
    fn shape(c: char) -> Patch {
        let row = match c {
            '+' => 25,
            c => c.to_digit(10).unwrap() * 2 + 2,
        };
        let mut px = vec![0u8; 900];
        for x in 0..30 {
            px[(row * 30 + x) as usize] = 1;
            px[((row + 1) * 30 + x) as usize] = 1;
        }
        Patch { w: 30, h: 30, px }
    }

    /// 見えている数字 `truth` を、`text` の `?` の所だけ読めなかったものとして作る（推測もできなかった）
    fn rd(text: &str, truth: &str) -> GlyphRead {
        rdg(text, text, truth)
    }

    /// `guess` は手がかりの数字での推測
    fn rdg(text: &str, guess: &str, truth: &str) -> GlyphRead {
        let mut chars = Vec::new();
        let mut glyphs = Vec::new();
        for (c, tc) in text.chars().zip(truth.chars()) {
            glyphs.push(match tc {
                '.' => Glyph::Dot,
                '-' if text.starts_with('-') && chars.is_empty() => Glyph::Minus,
                tc => Glyph::Shape(shape(tc)),
            });
            chars.push((c, if c == '?' { 0.5 } else { 0.95 }));
        }
        GlyphRead { text: text.into(), chars, guess: guess.into(), glyphs }
    }

    struct Run {
        l: Learner,
        t: Templates,
        at: DateTime<Utc>,
        got: Vec<Learned>,
    }

    impl Run {
        fn new() -> Self {
            // 時間帯の切り替わり（奇数時）から十分離す
            let at = DateTime::parse_from_rfc3339("2026-10-05T12:00:00Z").unwrap().with_timezone(&Utc);
            Run { l: Learner::default(), t: Templates::default(), at, got: Vec::new() }
        }

        fn feed(&mut self, seen: Seen, nums: &[(&'static str, GlyphRead)], n: u32) -> &mut Self {
            for _ in 0..n {
                let got = self.l.feed(self.at, &seen, nums, &self.t);
                self.got.extend(got);
                self.at += Duration::milliseconds(500);
            }
            self
        }

        fn wait(&mut self, secs: i64) -> &mut Self {
            self.at += Duration::seconds(secs);
            self
        }

        fn labels(&self) -> Vec<(Pool, String)> {
            self.got.iter().map(|l| (l.pool, l.label.clone())).collect()
        }
    }

    #[test]
    fn menu_digit_is_filled_from_the_result_of_the_match() {
        let mut r = Run::new();
        r.feed(Seen::RuleIntro(Rule::Area), &[], 4).wait(200);
        r.feed(Seen::Unknown, &[("power_number", rd("2100.0", "2100.0"))], 4);
        r.feed(Seen::Unknown, &[("power_number", rd("2147.3", "2147.3"))], 4).wait(60);
        r.feed(Seen::Unknown, &[("menu_x_value", rd("21?7.3", "2147.3"))], 4);
        assert_eq!(r.labels(), vec![(Pool::DigitMenu, "4".into())]);
        assert_eq!(r.got[0].patch.px, shape('4').px);
    }

    #[test]
    fn menu_before_the_match_is_not_the_same_value() {
        let mut r = Run::new();
        r.feed(Seen::Unknown, &[("menu_x_value", rd("21?0.0", "2150.0"))], 4).wait(30);
        r.feed(Seen::RuleIntro(Rule::Area), &[], 4).wait(200);
        r.feed(Seen::Unknown, &[("power_number", rd("2100.0", "2100.0"))], 4);
        assert!(r.got.is_empty());
    }

    #[test]
    fn x_power_of_another_rule_slot_is_not_the_same_value() {
        let mut r = Run::new();
        r.wait(3400); // 12:56 に試合後、13:00 過ぎにメニュー（ルールが変わっている）
        r.feed(Seen::Unknown, &[("power_number", rd("2100.0", "2100.0"))], 4).wait(200);
        r.feed(Seen::Unknown, &[("menu_x_value", rd("21?0.0", "2150.0"))], 4);
        assert!(r.got.is_empty());
    }

    #[test]
    fn udemae_gauge_digit_is_filled_by_the_total() {
        let mut r = Run::new();
        let tot = ("udemae_total", rd("380", "380"));
        r.feed(Seen::Unknown, &[("udemae_value", rd("-15", "-15")), tot.clone()], 4);
        r.feed(Seen::Unknown, &[("udemae_value", rd("3?5", "365")), tot], 4);
        assert_eq!(r.labels(), vec![(Pool::DigitGauge, "6".into())]);
    }

    #[test]
    fn total_digit_is_filled_by_the_gauge() {
        let mut r = Run::new();
        let tot = ("udemae_total", rd("3?0", "380"));
        r.feed(Seen::Unknown, &[("udemae_value", rd("-15", "-15")), tot.clone()], 4);
        r.feed(Seen::Unknown, &[("udemae_value", rd("365", "365")), tot], 4);
        assert_eq!(r.labels(), vec![(Pool::DigitTotal, "8".into())]);
    }

    #[test]
    fn x_power_digit_is_filled_by_the_delta() {
        let mut r = Run::new();
        let d = ("power_delta", rd("+62.2", "+62.2"));
        r.feed(Seen::Unknown, &[("power_number", rd("2194.6", "2194.6")), d.clone()], 4);
        r.feed(Seen::Unknown, &[("power_number", rd("2256.?", "2256.8")), d], 4);
        assert_eq!(r.labels(), vec![(Pool::Digit, "8".into())]);
    }

    #[test]
    fn moving_numbers_are_not_used() {
        let mut r = Run::new();
        let tot = ("udemae_total", rd("380", "380"));
        r.feed(Seen::Unknown, &[("udemae_value", rd("-15", "-15")), tot.clone()], 4);
        // 数え上がっている途中（2 回ずつしか同じでない）
        for v in ["1?0", "2?0", "3?5"] {
            r.feed(Seen::Unknown, &[("udemae_value", rd(v, "365")), tot.clone()], 2);
        }
        assert!(r.got.is_empty());
    }

    #[test]
    fn known_digits_must_agree_with_the_answer() {
        let mut r = Run::new();
        r.feed(Seen::Unknown, &[("power_number", rd("2147.3", "2147.3"))], 4).wait(30);
        r.feed(Seen::Unknown, &[("menu_x_value", rd("21?8.3", "2148.3"))], 4);
        assert!(r.got.is_empty());
    }

    #[test]
    fn too_many_unknown_digits_are_not_filled() {
        let mut r = Run::new();
        r.feed(Seen::Unknown, &[("udemae_value", rd("1051", "1051"))], 4).wait(30);
        r.feed(Seen::Unknown, &[("menu_udemae_value", rd("1???", "1051"))], 4);
        assert!(r.got.is_empty());
    }

    #[test]
    fn with_no_templates_guesses_are_kept_when_the_sum_is_right() {
        let mut r = Run::new();
        let d = ("power_delta", rdg("???.?", "+62.2", "+62.2"));
        r.feed(Seen::Unknown, &[("power_number", rdg("????.?", "2194.6", "2194.6")), d.clone()], 4);
        r.feed(Seen::Unknown, &[("power_number", rdg("????.?", "2256.8", "2256.8")), d], 4);
        let mut got: Vec<String> = r.got.iter().map(|l| format!("{}:{}", l.pool.dir_name(), l.label)).collect();
        got.sort();
        // 同じ形の字はまとめる（2 は 2 か所、6 は大きな数字と増減の両方）
        assert_eq!(
            got,
            ["digit:1", "digit:2", "digit:4", "digit:5", "digit:6", "digit:8", "digit:9", "digit_small:2", "digit_small:6", "digit_small:plus"]
        );
    }

    #[test]
    fn a_wrong_guess_breaks_the_sum_and_nothing_is_kept() {
        let mut r = Run::new();
        let d = ("power_delta", rdg("+62.2", "+62.2", "+62.2"));
        r.feed(Seen::Unknown, &[("power_number", rdg("21?4.6", "2194.6", "2194.6")), d.clone()], 4);
        // 8 を 3 と推測した（本当は 2256.8）
        r.feed(Seen::Unknown, &[("power_number", rdg("2256.?", "2256.3", "2256.8")), d], 4);
        // 動いた後の 8 は推測と食い違うので足さない。動く前の 9 は確かな 2 つ（動いた後は読めていない）が無いので足さない
        assert!(r.got.is_empty(), "{:?}", r.labels());
    }

    #[test]
    fn a_guess_against_the_answer_stops_the_fill() {
        let mut r = Run::new();
        r.feed(Seen::Unknown, &[("power_number", rd("2147.3", "2147.3"))], 4).wait(30);
        r.feed(Seen::Unknown, &[("menu_x_value", rdg("21?7.3", "2197.3", "2147.3"))], 4);
        assert!(r.got.is_empty());
    }

    #[test]
    fn a_glyph_like_another_digit_is_not_added() {
        let dir = std::env::temp_dir().join(format!("srw-learn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut r = Run::new();
        r.t = Templates::load(&dir).unwrap();
        // メニューの「4」の形が、大きな数字の「9」の見本と同じ → 答えを疑う
        r.t.add(Pool::Digit, "9", shape('4')).unwrap();
        r.feed(Seen::Unknown, &[("power_number", rd("2147.3", "2147.3"))], 4).wait(30);
        r.feed(Seen::Unknown, &[("menu_x_value", rd("21?7.3", "2147.3"))], 4);
        assert!(r.got.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 手元の見本（samples/snaps）で、数字の見本が 1 つも無いところから、画面を見るだけで見本ができるか試す。
/// `cargo test --release -- --ignored learns_from_snaps --nocapture`
#[cfg(test)]
mod with_samples {
    use super::*;
    use crate::recognize::Recognizer;

    fn load(key: &str) -> image::RgbImage {
        let dir = crate::samples_dir().join("snaps");
        let p = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "png") && !p.to_string_lossy().ends_with("_ほこ.png"))
            .find(|p| p.file_name().unwrap().to_string_lossy().contains(key))
            .unwrap();
        image::open(p).unwrap().to_rgb8()
    }

    #[test]
    #[ignore]
    fn learns_from_snaps() {
        // 手元の見本から、数字以外（見出しなど）だけを写した置き場所
        let dir = std::env::temp_dir().join(format!("srw-learn-snaps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = Templates::load(&Templates::default_dir()).unwrap();
        let mut t = Templates::load(&dir).unwrap();
        for pool in Pool::ALL.into_iter().filter(|p| !DIGIT_POOLS.contains(p)) {
            for tm in src.get(pool) {
                t.add(pool, &tm.label, tm.patch.clone()).unwrap();
            }
        }
        let mut rec = Recognizer::new(t);
        for frames in [["101412", "101431"], ["040820", "040905"]] {
            let mut l = Learner::default();
            let mut at = chrono::Utc::now();
            for key in frames {
                let img = load(key);
                for _ in 0..4 {
                    let r = rec.recognize(&img);
                    let nums: Vec<String> = r.numbers.iter().map(|(p, g)| format!("{p}={}/{}", g.text, g.guess)).collect();
                    let got = l.feed(at, &r.seen, &r.numbers, rec.templates());
                    for g in &got {
                        println!("  足す {} {}: {}", g.pool.dir_name(), g.label, g.why);
                    }
                    for g in got {
                        rec.templates_mut().add_auto(g.pool, &g.label, g.patch).unwrap();
                    }
                    at += Duration::milliseconds(500);
                    if at.timestamp_subsec_millis() < 500 {
                        println!("{key}: {:?} {}", r.seen, nums.join(" "));
                    }
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
