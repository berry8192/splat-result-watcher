//! 照合。登録した見本（templates.rs）で、1 フレームに何が見えたか（[`Seen`]）を決める。
//!
//! しきい値は見本 76 枚で試した値（2026-10-03、`probe`）。録画が貯まったら測り直す。
//! - 決まった文字の絵: 一番近いラベルの一致度がしきい値以上で、2 番目より `MARGIN` 以上高いこと
//! - 数字: 1 文字ずつ一番近い字を取り、どの字も `GLYPH_MIN` 以上で、形（桁数と小数 1 桁）が合うこと
//!
//! まだ読まないもの: 個人リザルトの「無効試合」「負けとして…」の文言、進行、計測中、バンカラの参加費。
//! 見本の登録の場所を足してから読む。

use image::RgbImage;
use serde::Serialize;

use crate::matching::{self, Glyph};
use crate::nair;
use crate::state::{Mode, Note, Observed, Outcome, Rule, Seen};
use crate::templates::{self, glyph_char, place, Place, Templates};
use crate::NO_SIGNAL_DARK;

/// ずらして探す幅（照合する大きさでの px）
const SHIFT: u32 = 4;
/// 2 番目のラベルとの差
const MARGIN: f64 = 0.2;
const RULE_INTRO_MIN: f64 = 0.6;
const OUTCOME_MIN: f64 = 0.6;
/// モードの見出しは字が小さく、白黒にすると線が途切れやすい（ぼけた見本で 0.35）
const MODE_MIN: f64 = 0.4;
const RULE_MIN: f64 = 0.6;
const POWER_LABEL_MIN: f64 = 0.6;
const MATCHING_MIN: f64 = 0.6;
const UDEMAE_TITLE_MIN: f64 = 0.6;
/// メニューの見出しは小さな字で崩れやすい（「ウデマエ」は別の画面の見本と 0.52）。数字の形でも確かめるので低めでよい
const MENU_LABEL_MIN: f64 = 0.4;
/// 数字の 1 文字（正しい字は 0.87 以上、2 番目に近い字は 0.79 以下だった）
pub const GLYPH_MIN: f64 = 0.85;

/// ラベルごとの一番よい一致度（高い順）
#[derive(Clone, Debug, Serialize)]
pub struct Score {
    pub label: String,
    pub score: f64,
}

/// 数字を読んだ結果
#[derive(Clone, Debug, Serialize)]
pub struct GlyphRead {
    /// 読めた文字列。見本の無い字・一致度の足りない字は `?`
    pub text: String,
    /// 1 文字ずつ: (読んだ字, 一致度)。小数点とマイナスは形で決まるので (`.` / `-`, 1.0)
    pub chars: Vec<(char, f64)>,
}

/// 1 フレームを読んだ結果。`notes` は GUI に見せる途中経過、`peaks` は記録に残す一致度
/// （場所の短い名前, 一番近いラベルか読んだ数字, 一致度。数字は 1 文字ずつの一致度の一番低いもの）
#[derive(Clone, Debug)]
pub struct Reading {
    pub seen: Seen,
    pub notes: Vec<String>,
    pub peaks: Vec<(String, String, f64)>,
}

pub struct Recognizer {
    t: Templates,
}

/// 一番よいものと 2 番目
fn top2(scores: &[Score]) -> (Option<&Score>, f64) {
    (scores.first(), scores.get(1).map_or(0.0, |s| s.score))
}

/// 途中経過を貯める
#[derive(Default)]
struct Notes {
    text: Vec<String>,
    peaks: Vec<(String, String, f64)>,
}

impl Notes {
    fn number(&mut self, short: &str, r: &GlyphRead) {
        self.text.push(format!("{short}: {}", r.note()));
        let low = r.chars.iter().map(|c| c.1).fold(1.0, f64::min);
        self.peaks.push((short.to_string(), r.text.clone(), if r.chars.is_empty() { 0.0 } else { low }));
    }
}

impl GlyphRead {
    /// 途中経過に出す形（読み と 1 文字ずつの一致度）
    pub fn note(&self) -> String {
        let each: Vec<String> = self.chars.iter().filter(|(c, _)| *c != '.' && *c != '-').map(|(c, v)| format!("{c}{v:.2}")).collect();
        format!("{}（{}）", self.text, each.join(" "))
    }
}

impl Recognizer {
    pub fn new(t: Templates) -> Self {
        Recognizer { t }
    }

    pub fn templates(&self) -> &Templates {
        &self.t
    }

    pub fn templates_mut(&mut self) -> &mut Templates {
        &mut self.t
    }

    /// その場所をラベルの見本と比べ、ラベルごとの一番よい一致度を高い順に返す
    pub fn scores(&self, work: &RgbImage, place: &Place) -> Vec<Score> {
        let list = self.t.get(place.pool);
        if list.is_empty() {
            return Vec::new();
        }
        let sample = templates::cut_margin(work, place, SHIFT);
        let mut best: Vec<Score> = Vec::new();
        for t in list {
            let v = matching::iou(&t.patch, &sample);
            match best.iter_mut().find(|s| s.label == t.label) {
                Some(s) => s.score = s.score.max(v),
                None => best.push(Score { label: t.label.clone(), score: v }),
            }
        }
        best.sort_by(|a, b| b.score.total_cmp(&a.score));
        best
    }

    /// しきい値と差で 1 つに決める
    fn decide(&self, work: &RgbImage, place: &Place, min: f64, notes: &mut Notes) -> Option<String> {
        let scores = self.scores(work, place);
        let (first, second) = top2(&scores);
        let first = first?;
        notes.text.push(format!("{}: {} 一致度 {:.2}（2 番目 {:.2}）", place.short, first.label, first.score, second));
        notes.peaks.push((place.short.to_string(), first.label.clone(), first.score));
        (first.score >= min && first.score - second >= MARGIN).then(|| first.label.clone())
    }

    /// 切り出した 1 文字ずつ（GUI で見せる・登録する）
    pub fn glyphs(&self, work: &RgbImage, place: &Place) -> Vec<Glyph> {
        templates::cut_glyphs(work, place)
    }

    /// 数字を読む
    pub fn read_glyphs(&self, work: &RgbImage, place: &Place) -> GlyphRead {
        let list = self.t.get(place.pool);
        let mut text = String::new();
        let mut chars = Vec::new();
        for g in self.glyphs(work, place) {
            let g = match g {
                Glyph::Dot | Glyph::Minus => {
                    let c = if matches!(g, Glyph::Dot) { '.' } else { '-' };
                    text.push(c);
                    chars.push((c, 1.0));
                    continue;
                }
                Glyph::Shape(g) => g,
            };
            let mut best: Option<(char, f64)> = None;
            for t in list {
                let Some(c) = glyph_char(&t.label) else { continue };
                let v = matching::glyph_iou(&g, &t.patch);
                if best.is_none_or(|b| v > b.1) {
                    best = Some((c, v));
                }
            }
            match best {
                Some((c, v)) if v >= GLYPH_MIN => {
                    text.push(c);
                    chars.push((c, v));
                }
                Some((c, v)) => {
                    text.push('?');
                    chars.push((c, v));
                }
                None => {
                    text.push('?');
                    chars.push(('?', 0.0));
                }
            }
        }
        GlyphRead { text, chars }
    }

    /// 1 フレームを読む。`game` はゲーム穴（大きさは問わない）
    pub fn recognize(&self, game: &RgbImage) -> Reading {
        let mut notes = Notes::default();
        if nair::dark_ratio(game) > NO_SIGNAL_DARK {
            return Reading { seen: Seen::NoSignal, notes: Vec::new(), peaks: Vec::new() };
        }
        let work = templates::to_work(game);
        let seen = self.recognize_work(&work, &mut notes);
        Reading { seen, notes: notes.text, peaks: notes.peaks }
    }

    fn recognize_work(&self, work: &RgbImage, notes: &mut Notes) -> Seen {
        let p = |id| place(id).expect("場所の名前");
        let rule = |l: String| match l.as_str() {
            "area" => Some(Rule::Area),
            "yagura" => Some(Rule::Yagura),
            "hoko" => Some(Rule::Hoko),
            "asari" => Some(Rule::Asari),
            _ => None,
        };

        // 試合は必ずルール紹介から始まる
        if let Some(r) = self.decide(work, p("rule_intro"), RULE_INTRO_MIN, notes).and_then(rule) {
            return Seen::RuleIntro(r);
        }

        if let Some(l) = self.decide(work, p("outcome"), OUTCOME_MIN, notes) {
            match l.as_str() {
                "win" => return Seen::Outcome(Outcome::Win),
                "lose" => return Seen::Outcome(Outcome::Lose),
                _ => {}
            }
        }

        if let Some(l) = self.decide(work, p("mode"), MODE_MIN, notes) {
            let mode = match l.as_str() {
                "x" => Mode::X,
                "bankara_challenge" => Mode::BankaraChallenge,
                "bankara_open" => Mode::BankaraOpen,
                _ => Mode::Other,
            };
            let rule = self.decide(work, p("rule"), RULE_MIN, notes).and_then(rule);
            return Seen::Header { mode, rule, note: Note::None };
        }

        if let Some(l) = self.decide(work, p("matching"), MATCHING_MIN, notes) {
            // 「ウデマエ」ではチャレンジとオープンを見分けられない（オープンは当面対応しない）
            return Seen::Matching(if l == "x" { Mode::X } else { Mode::BankaraChallenge });
        }

        if let Some(l) = self.decide(work, p("udemae_title"), UDEMAE_TITLE_MIN, notes) {
            if l == "promoted" {
                let n = self.read_glyphs(work, p("udemae_reset"));
                notes.number("リセット", &n);
                if let Some(v) = parse_points(&n.text) {
                    return Seen::UdemaeReset(v);
                }
            } else {
                let n = self.read_glyphs(work, p("udemae_value"));
                notes.number("ウデマエ", &n);
                if let Some(value) = parse_points(&n.text) {
                    let t = self.read_glyphs(work, p("udemae_total"));
                    notes.number("TOTAL", &t);
                    return Seen::Udemae { value, total: parse_points(&t.text) };
                }
            }
        }

        // ロビーのメニューに出ている自分の値（observed）
        if self.decide(work, p("menu_x_label"), MENU_LABEL_MIN, notes).is_some() {
            let n = self.read_glyphs(work, p("menu_x_value"));
            notes.number("メニューの X パワー", &n);
            if let Some(value) = parse_power(&n.text) {
                return Seen::Observed { what: Observed::X { rule: None, value }, wins: None, losses: None };
            }
        }
        if self.decide(work, p("menu_udemae_label"), MENU_LABEL_MIN, notes).is_some() {
            let n = self.read_glyphs(work, p("menu_udemae_value"));
            notes.number("メニューのウデマエ", &n);
            if let Some(value) = parse_points(&n.text) {
                return Seen::Observed { what: Observed::Udemae { value }, wins: None, losses: None };
            }
        }

        let label = self.scores(work, p("power_label"));
        if let Some(s) = label.first() {
            notes.text.push(format!("「Xパワー」: 一致度 {:.2}", s.score));
            notes.peaks.push(("「Xパワー」".into(), s.label.clone(), s.score));
            if s.score >= POWER_LABEL_MIN {
                let n = self.read_glyphs(work, p("power_number"));
                notes.number("Xパワー", &n);
                if let Some(value) = parse_power(&n.text) {
                    // 増減は任意（見本があれば念押しに使う。無ければ旧値から新値へ動いたのを見届けて出す）
                    let d = self.read_glyphs(work, p("power_delta"));
                    notes.number("増減", &d);
                    return Seen::XPower { value, delta: parse_delta(&d.text) };
                }
            }
        }
        Seen::Unknown
    }
}

/// X パワーの下限（これより下がらない。2026-10 に調べた仕様）。下回る読みは読み違い
pub const X_POWER_MIN: f64 = 500.0;

/// 「2194.6」: 整数 3〜4 桁と小数 1 桁で、500 以上
pub fn parse_power(s: &str) -> Option<f64> {
    let (i, f) = s.split_once('.')?;
    let ok = (3..=4).contains(&i.len())
        && f.len() == 1
        && i.bytes().chain(f.bytes()).all(|b| b.is_ascii_digit());
    let v: f64 = if ok { s.parse().ok()? } else { return None };
    (v >= X_POWER_MIN).then_some(v)
}

/// ウデマエポイント「130」「-15」「1051」（p は切り出しで捨ててある）。
/// 仕様の範囲は −9999〜9999 なので、4 桁までに限ることがそのまま範囲の確かめになる
pub fn parse_points(s: &str) -> Option<i32> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let ok = (1..=4).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| s.parse().ok()).flatten()
}

/// 「+94.6」「-12.0」
pub fn parse_delta(s: &str) -> Option<f64> {
    let sign = match s.chars().next()? {
        '+' => 1.0,
        '-' => -1.0,
        _ => return None,
    };
    let (i, f) = s[1..].split_once('.')?;
    let ok = (1..=3).contains(&i.len())
        && f.len() == 1
        && i.bytes().chain(f.bytes()).all(|b| b.is_ascii_digit());
    ok.then(|| s[1..].parse::<f64>().ok().map(|v| sign * v)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_forms() {
        assert_eq!(parse_power("2194.6"), Some(2194.6));
        assert_eq!(parse_power("830.0"), Some(830.0));
        assert_eq!(parse_power("21946"), None);
        assert_eq!(parse_power("2?94.6"), None);
        assert_eq!(parse_power("2194.62"), None);
        assert_eq!(parse_power("499.9"), None, "X パワーは 500 より下がらない");
        assert_eq!(parse_power("500.0"), Some(500.0));
        assert_eq!(parse_delta("+94.6"), Some(94.6));
        assert_eq!(parse_delta("-117.0"), Some(-117.0));
        assert_eq!(parse_delta("+622"), None);
        assert_eq!(parse_delta("94.6"), None);
        assert_eq!(parse_points("-15"), Some(-15));
        assert_eq!(parse_points("1051"), Some(1051));
        assert_eq!(parse_points("3?0"), None);
        assert_eq!(parse_points("-"), None);
        assert_eq!(parse_points("-9999"), Some(-9999));
        assert_eq!(parse_points("10000"), None, "ウデマエポイントは 9999 まで");
    }
}

/// 手元の見本（`samples/snaps/`、git 管理外）で、登録から照合までを通して試す。
/// `cargo test --release -- --ignored` で動かす
#[cfg(test)]
mod with_samples {
    use super::*;
    use crate::templates::{cut, glyph_label, to_work};

    fn load(key: &str) -> RgbImage {
        let dir = crate::samples_dir().join("snaps");
        let p = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "png"))
            .filter(|p| !p.to_string_lossy().ends_with("_ほこ.png"))
            .find(|p| p.file_name().unwrap().to_string_lossy().contains(key))
            .unwrap_or_else(|| panic!("見本 {key} が無い"));
        to_work(&image::open(p).unwrap().to_rgb8())
    }

    #[test]
    #[ignore]
    fn register_then_recognize() {
        let dir = std::env::temp_dir().join(format!("srw-rec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = Templates::load(&dir).unwrap();
        for (place_id, key, label) in [
            ("outcome", "032757", "win"),
            ("outcome", "034734", "lose"),
            ("mode", "032822", "x"),
            ("mode", "042128", "bankara_challenge"),
            ("rule", "032822", "yagura"),
            ("rule", "035159", "area"),
            ("rule", "042128", "asari"),
            ("rule", "050430", "hoko"),
            ("power_label", "033034", "x_power"),
            ("rule_intro", "040228", "area"),
            ("rule_intro", "132222", "yagura"),
            ("rule_intro", "041551", "hoko"),
            ("rule_intro", "040958", "asari"),
            ("udemae_title", "040440", "finish"),
            ("udemae_title", "040820", "clear"),
            ("udemae_title", "041716", "promoted"),
            ("matching", "032333", "x"),
            ("matching", "040151", "bankara"),
            ("menu_x_label", "031924", "x_power"),
            ("menu_udemae_label", "040042", "udemae"),
        ] {
            let p = place(place_id).unwrap();
            t.add(p.pool, label, cut(&load(key), p)).unwrap();
        }
        for (place_id, key, text) in [
            ("power_number", "101431", "2256.8"),
            ("power_number", "033316", "2139.4"),
            ("power_number", "101601", "2265.9"),
            ("calibrated_number", "041847", "1830.4"),
            ("power_delta", "101601", "+25.0"),
            ("power_delta", "101431", "+62.2"),
            ("power_delta", "033306", "+94.6"),
            ("udemae_value", "040440", "130"),
            ("udemae_value", "041132", "685"),
            ("udemae_value", "040820", "-15"),
            ("udemae_value", "040905", "365"),
            ("udemae_total", "040526", "25"),
            ("udemae_total", "040905", "380"),
            ("menu_x_value", "031924", "2100.0"),
            ("menu_udemae_value", "040042", "300"),
            ("menu_udemae_value", "041221", "1051"),
        ] {
            let p = place(place_id).unwrap();
            let g = templates::cut_glyphs(&load(key), p);
            let kinds: Vec<String> = g
                .iter()
                .map(|g| match g {
                    Glyph::Dot => ".".into(),
                    Glyph::Minus => "-".into(),
                    Glyph::Shape(p) => format!("字{}", p.px.iter().map(|&v| v as u32).sum::<u32>()),
                })
                .collect();
            assert_eq!(g.len(), text.chars().count(), "{key} の切れ方 {kinds:?}");
            for (c, g) in text.chars().zip(g) {
                if let (Some(l), Glyph::Shape(g)) = (glyph_label(c), g) {
                    t.add(p.pool, &l, g).unwrap();
                }
            }
        }
        let r = Recognizer::new(Templates::load(&dir).unwrap());
        let see = |key: &str| r.recognize(&load(key)).seen;
        assert_eq!(see("032745"), Seen::Outcome(Outcome::Win), "ステッカーの重なった WIN!");
        assert_eq!(see("042142"), Seen::Header { mode: Mode::BankaraChallenge, rule: Some(Rule::Asari), note: Note::None });
        assert_eq!(see("033345"), Seen::XPower { value: 2194.6, delta: Some(94.6) });
        // 7 の見本が無いので増減は読めない
        assert_eq!(see("101625"), Seen::XPower { value: 2336.8, delta: None });
        assert_eq!(see("132222"), Seen::RuleIntro(Rule::Yagura));
        // バンカラの精算: 見本にしなかった 155（130・685・-15・365 の見本から）と、TOTAL（大きな数字の見本）
        let why = |key: &str| r.recognize(&load(key)).notes.join(" / ");
        assert_eq!(see("040535"), Seen::Udemae { value: 155, total: Some(25) }, "{}", why("040535"));
        assert_eq!(see("040526"), Seen::Udemae { value: 130, total: Some(25) }, "{}", why("040526"));
        assert_eq!(see("040905"), Seen::Udemae { value: 365, total: Some(380) }, "{}", why("040905"));
        assert_eq!(see("041716"), Seen::UdemaeReset(300), "{}", why("041716"));
        assert_eq!(see("032333"), Seen::Matching(Mode::X));
        // メニューの値（手元の見本では字がそろわないので、見本にした画面を読んで仕組みが通るかだけ確かめる）
        let menu_x = Seen::Observed { what: Observed::X { rule: None, value: 2100.0 }, wins: None, losses: None };
        assert_eq!(see("031924"), menu_x, "{}", why("031924"));
        let menu_ud = Seen::Observed { what: Observed::Udemae { value: 1051 }, wins: None, losses: None };
        assert_eq!(see("041221"), menu_ud, "{}", why("041221"));
        assert_eq!(see("040151"), Seen::Matching(Mode::BankaraChallenge));
        // メニュー・順位・試合中（無効試合の札・バトル中・Finish!）・X に挑戦できる・進行
        let quiet = [
            "033416", "042113", "041735", "041804", "134014", "134030", "134042", "134056", "040302", "041437",
        ];
        for key in quiet {
            assert_eq!(see(key), Seen::Unknown, "{key} は何でもない");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
