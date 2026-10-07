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

use crate::matching::{self, Glyph, Patch, Roi};
use crate::nair;
use crate::shapes;
use crate::starter;
use crate::state::{Mode, Note, Observed, Outcome, Rule, Seen};
use crate::templates::{self, glyph_char, place, Place, Pool, Templates};
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
const PROGRESS_LABEL_MIN: f64 = 0.6;

/// 進行の○とイカを色で数える場所（基準 1536×864）と、かたまりの幅の目安（2026-10-04 に見本 10 枚で合った）
pub(crate) struct Strip {
    stamps: Roi,
    stamp_w: u32,
    squids: Roi,
    squid_w: u32,
}

/// 試合後の進行の画面（X・昇格戦の○ 3 個も、チャレンジの 5 個も入る幅）
const PROGRESS_STRIP: Strip = Strip { stamps: Roi::new(540, 415, 460, 90), stamp_w: 83, squids: Roi::new(555, 520, 290, 60), squid_w: 36 };
/// ロビーのメニュー（X）。判子の付いた見本はまだ無いので、幅は○の大きさからの見積もり
pub(crate) const MENU_X_STRIP: Strip = Strip { stamps: Roi::new(1310, 300, 190, 60), stamp_w: 50, squids: Roi::new(1325, 362, 130, 36), squid_w: 20 };
/// ロビーのメニュー（バンカラ。チャレンジの○ 5 個・昇格戦の○ 3 個とも入る幅）
pub(crate) const MENU_BANKARA_STRIP: Strip =
    Strip { stamps: Roi::new(1215, 300, 290, 60), stamp_w: 50, squids: Roi::new(1225, 362, 240, 36), squid_w: 20 };

/// WIN の判子（黄緑）
fn is_stamp([r, g, b]: [u8; 3]) -> bool {
    r >= 170 && g >= 180 && b <= 110
}

/// 残っているイカ（橙。負けたイカは灰色に ×）
fn is_squid([r, g, b]: [u8; 3]) -> bool {
    r >= 190 && (70..=170).contains(&g) && b <= 90
}

/// 勝ち数（判子の数）と負け数（3 − 残っているイカ）。イカが 3 匹より多く見えたら数えない
pub(crate) fn count_progress(work: &RgbImage, s: &Strip) -> Option<(u8, u8)> {
    let range = |w: u32| (w / 2, w * 8 / 5);
    let (a, b) = range(s.stamp_w);
    let wins = matching::count_color_runs(work, s.stamps, is_stamp, a, b);
    let (a, b) = range(s.squid_w);
    let squids = matching::count_color_runs(work, s.squids, is_squid, a, b);
    (wins <= 5 && squids <= 3).then_some((wins as u8, 3 - squids as u8))
}
/// メニューの勝ち負けのランプ。残機（矢印・イカ）が 1 つも無いことはない（3 敗でセットが終わり、次のセットは満タン）ので、
/// 1 つも見えなければ隠れているとみて数えない
fn menu_progress(work: &RgbImage, s: &Strip, notes: &mut Notes) -> Option<(u8, u8)> {
    let wl = count_progress(work, s).filter(|&(_, l)| l < 3);
    notes.text.push(format!("メニューの進行: {wl:?}"));
    wl
}

/// 数字の 1 文字（正しい字は 0.87 以上、2 番目に近い字は 0.79 以下だった）
pub const GLYPH_MIN: f64 = 0.85;
/// S+ の小さな数字: 見本の一致度の下限と 2 番目の字との差、手がかりの数字の下限
const RANK_DIGIT_MIN: f64 = 0.45;
const RANK_DIGIT_MARGIN: f64 = 0.1;
const RANK_STARTER_MIN: f64 = 0.6;

/// ラベルごとの一番よい一致度（高い順）
#[derive(Clone, Debug, Serialize)]
pub struct Score {
    pub label: String,
    pub score: f64,
}

/// 数字を読んだ結果
/// 推測を読みとして使ってよい一致度と、2 番目との差（精算の増減・参加費の後の値の大きな字だけ。2026-10-06 の録画の
/// 「-13」「20」「80」「277」は、正しい字が 0.70〜0.93、2 番目との差 0.09〜0.46 だった）
const SURE_GUESS_MIN: f64 = 0.65;
const SURE_GUESS_MARGIN: f64 = 0.08;

/// 見本で読めない字が、どれも手がかりの数字ではっきり決まるなら、その推測。テンプレートには足さない
/// （足すのは計算で確かめられたものだけ。learn.rs）
fn sure_guess(place: &Place, r: &GlyphRead) -> Option<String> {
    for ((c, _), g) in r.text.chars().zip(&r.chars).zip(&r.glyphs) {
        if c != '?' {
            continue;
        }
        let Glyph::Shape(g) = g else { return None };
        let s = starter::guess(place.pool, g)?;
        if s.score < SURE_GUESS_MIN || s.margin < SURE_GUESS_MARGIN {
            return None;
        }
    }
    (!r.guess.contains('?')).then(|| r.guess.clone())
}

#[derive(Clone, Debug, Serialize)]
pub struct GlyphRead {
    /// 読めた文字列。見本の無い字・一致度の足りない字は `?`
    pub text: String,
    /// 1 文字ずつ: (読んだ字, 一致度)。小数点とマイナスは形で決まるので (`.` / `-`, 1.0)
    pub chars: Vec<(char, f64)>,
    /// `text` の `?` を、手がかりの数字（starter.rs）で推測して埋めたもの。推測できない字は `?` のまま。
    /// 確かな読みとしては使わず、計算で確かめてから見本にする（learn.rs）
    pub guess: String,
    /// 切り出した 1 文字ずつ（`chars` と同じ並び。見本を自動で足すときに使う）
    #[serde(skip)]
    pub glyphs: Vec<Glyph>,
}

/// 形と色で見分けた見出し（見本はまだ無い）。確かめられたら、この切り出しを見本にする
#[derive(Clone, Debug)]
pub struct ShapeLabel {
    pub place: &'static str,
    pub label: &'static str,
    pub patch: Patch,
}

/// 1 フレームを読んだ結果。`notes` は GUI に見せる途中経過、`peaks` は記録に残す一致度
/// （場所の短い名前, 一番近いラベルか読んだ数字, 一致度。数字は 1 文字ずつの一致度の一番低いもの）
#[derive(Clone, Debug)]
pub struct Reading {
    pub seen: Seen,
    pub notes: Vec<String>,
    pub peaks: Vec<(String, String, f64)>,
    /// 読んだ数字（場所の id, 読み）。読めない字が混じっていても入る（learn.rs が使う）
    pub numbers: Vec<(&'static str, GlyphRead)>,
    /// 見本ではなく形と色で見分けた見出し
    pub shape_labels: Vec<ShapeLabel>,
}

impl Reading {
    pub fn no_signal() -> Self {
        Reading { seen: Seen::NoSignal, notes: Vec::new(), peaks: Vec::new(), numbers: Vec::new(), shape_labels: Vec::new() }
    }
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
    numbers: Vec<(&'static str, GlyphRead)>,
    shape_labels: Vec<ShapeLabel>,
}

impl Notes {
    /// 見本ではなく形と色で見分けた（その場所の切り出しを、確かめられたら見本にする。learn.rs）
    fn shape(&mut self, work: &RgbImage, place: &'static Place, label: &'static str) {
        self.text.push(format!("形で見分けた: {} {label}", place.short));
        self.shape_labels.push(ShapeLabel { place: place.id, label, patch: templates::cut(work, place) });
    }

    fn number(&mut self, place: &Place, short: &str, r: &GlyphRead) {
        self.numbers.push((place.id, r.clone()));
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
        let mut guess = String::new();
        let glyphs = self.glyphs(work, place);
        for (i, g) in glyphs.iter().enumerate() {
            let g = match g {
                Glyph::Dot | Glyph::Minus => {
                    let c = if matches!(g, Glyph::Dot) { '.' } else { '-' };
                    text.push(c);
                    guess.push(c);
                    chars.push((c, 1.0));
                    continue;
                }
                Glyph::Shape(g) => g,
            };
            let mut best: Option<(char, f64)> = None;
            for t in list {
                let Some(c) = glyph_char(&t.label) else { continue };
                let v = matching::glyph_iou(g, &t.patch);
                if best.is_none_or(|b| v > b.1) {
                    best = Some((c, v));
                }
            }
            match best {
                Some((c, v)) if v >= GLYPH_MIN => {
                    text.push(c);
                    guess.push(c);
                    chars.push((c, v));
                    continue;
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
            // 増減の先頭の形のある字は「+」（「-」は低いかたまりで、形で決まる）。手がかりの数字に「+」は無い
            let c = if place.id == "power_delta" && i == 0 {
                Some('+')
            } else {
                starter::guess(place.pool, g).map(|g| g.c)
            };
            guess.push(c.unwrap_or('?'));
        }
        GlyphRead { text, chars, guess, glyphs }
    }

    /// 1 フレームを読む。`game` はゲーム穴（大きさは問わない）
    pub fn recognize(&self, game: &RgbImage) -> Reading {
        let mut notes = Notes::default();
        if nair::dark_ratio(game) > NO_SIGNAL_DARK {
            return Reading::no_signal();
        }
        let work = templates::to_work(game);
        let seen = self.recognize_work(&work, &mut notes);
        Reading { seen, notes: notes.text, peaks: notes.peaks, numbers: notes.numbers, shape_labels: notes.shape_labels }
    }

    /// ロビーのメニューの X パワー。見出しが見本で決まらなければ、色と数字の形式で。
    /// `None` はメニューではない、`Some(None)` はメニューだが値が見本で読めない
    fn menu_x(&self, work: &RgbImage, notes: &mut Notes) -> Option<Option<(Observed, Option<u8>, Option<u8>)>> {
        let p = |id| place(id).expect("場所の名前");
        let by_label = self.decide(work, p("menu_x_label"), MENU_LABEL_MIN, notes).is_some();
        if !(by_label || shapes::menu_x_label(work)) {
            return None;
        }
        let n = self.read_glyphs(work, p("menu_x_value"));
        if !(by_label || parse_power(&n.guess).is_some()) {
            return None;
        }
        notes.number(p("menu_x_value"), "メニューの X パワー", &n);
        if !by_label {
            notes.shape(work, p("menu_x_label"), "x_power");
        }
        let wl = menu_progress(work, &MENU_X_STRIP, notes);
        let value = parse_power(&n.text);
        Some((value.is_some() || wl.is_some()).then(|| (Observed::X { rule: None, value }, wl.map(|w| w.0), wl.map(|w| w.1))))
    }

    /// ロビーのメニューのウデマエポイント（`menu_x` と同じ）
    fn menu_udemae(&self, work: &RgbImage, notes: &mut Notes) -> Option<Option<(Observed, Option<u8>, Option<u8>)>> {
        let p = |id| place(id).expect("場所の名前");
        let by_label = self.decide(work, p("menu_udemae_label"), MENU_LABEL_MIN, notes).is_some();
        if !(by_label || shapes::menu_udemae_label(work)) {
            return None;
        }
        let n = self.read_glyphs(work, p("menu_udemae_value"));
        if !(by_label || parse_points(&n.guess).is_some()) {
            return None;
        }
        notes.number(p("menu_udemae_value"), "メニューのウデマエ", &n);
        if !by_label {
            notes.shape(work, p("menu_udemae_label"), "udemae");
        }
        let wl = menu_progress(work, &MENU_BANKARA_STRIP, notes);
        let value = parse_points(&n.text);
        let rank = self.menu_rank(work);
        if let Some(r) = rank {
            notes.text.push(format!("ランク: {r}"));
        }
        Some((value.is_some() || wl.is_some() || rank.is_some()).then(|| (Observed::Udemae { value, rank }, wl.map(|w| w.0), wl.map(|w| w.1))))
    }

    /// メニューのウデマエのランク。S+ の数字は、メニューの数字の見本（なければ手がかりの数字）で読む。
    /// S+ の数字は照合する大きさで高さ 12 画素ほどしかなく、見本との一致度は 0.5 前後にとどまる
    /// （本番の「S+1」の 1 が 0.50、2 番目の字が 0.36）。2 番目の字と離れていれば採り、決まらなければ読まない
    fn menu_rank(&self, work: &RgbImage) -> Option<crate::rank::Rank> {
        let list = self.t.get(Pool::DigitMenu);
        crate::rank::read_menu(work, shapes::rank_letter, |g| {
            let Glyph::Shape(g) = g else { return None };
            // 字ごとに一番近い見本
            let mut by_char: Vec<(char, f64)> = Vec::new();
            for t in list {
                let Some(c) = glyph_char(&t.label).filter(|c| c.is_ascii_digit()) else { continue };
                let v = matching::glyph_iou(g, &t.patch);
                match by_char.iter_mut().find(|b| b.0 == c) {
                    Some(b) => b.1 = b.1.max(v),
                    None => by_char.push((c, v)),
                }
            }
            by_char.sort_by(|a, b| b.1.total_cmp(&a.1));
            let second = by_char.get(1).map_or(0.0, |b| b.1);
            match by_char.first() {
                Some(&(c, v)) if v >= GLYPH_MIN || (v >= RANK_DIGIT_MIN && v - second >= RANK_DIGIT_MARGIN) => Some(c),
                _ => starter::guess(Pool::DigitMenu, g).filter(|s| s.c.is_ascii_digit() && s.score >= RANK_STARTER_MIN).map(|s| s.c),
            }
        })
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
        let mode_label = |m: Mode| if m == Mode::X { "x" } else { "bankara_challenge" };

        // 試合は必ずルール紹介から始まる。見本は 2 行目の語なので、ナワバリバトルの「バトル」がガチホコに見える。
        // 1 行目が「ナワバリ」なら紹介とは見ない（ナワバリは数えない）
        if let Some(r) = self.decide(work, p("rule_intro"), RULE_INTRO_MIN, notes).and_then(rule).filter(|_| shapes::intro_frame(work)) {
            if r == Rule::Hoko && shapes::turf_intro(work) {
                notes.text.push("ナワバリバトルの紹介".into());
                return Seen::RuleIntro(Rule::TurfWar);
            }
            return Seen::RuleIntro(r);
        }
        if let Some(r) = shapes::rule_intro(work) {
            // ナワバリは見本のラベルに無い（ガチホコと 2 行目が同じなので、見本にもしない）
            if r != Rule::TurfWar {
                notes.shape(work, p("rule_intro"), r.as_str());
            }
            return Seen::RuleIntro(r);
        }

        let outcome = match self.decide(work, p("outcome"), OUTCOME_MIN, notes).as_deref() {
            Some("win") => Some(Outcome::Win),
            Some("lose") => Some(Outcome::Lose),
            _ => shapes::outcome(work)
                .inspect(|o| notes.shape(work, p("outcome"), if *o == Outcome::Win { "win" } else { "lose" })),
        };
        if let Some(o) = outcome {
            return Seen::Outcome(o);
        }

        let header = match self.decide(work, p("mode"), MODE_MIN, notes) {
            Some(l) => Some(match l.as_str() {
                "x" => Mode::X,
                "bankara_challenge" => Mode::BankaraChallenge,
                "bankara_open" => Mode::BankaraOpen,
                _ => Mode::Other,
            }),
            None => shapes::header_mode(work).inspect(|m| notes.shape(work, p("mode"), mode_label(*m))),
        };
        if let Some(mode) = header {
            let rule = self.decide(work, p("rule"), RULE_MIN, notes).and_then(rule);
            // 見出しの画面の下の方に、自分の表彰（金と銀の印）が並ぶ
            let medals = shapes::medals(work);
            if let Some((g, s)) = medals {
                notes.text.push(format!("形で見分けた: 表彰 金 {g} 銀 {s}"));
            }
            return Seen::Header { mode, rule, note: Note::None, medals };
        }

        let matching = match self.decide(work, p("matching"), MATCHING_MIN, notes) {
            // 見出しの「ウデマエ」ではチャレンジとオープンを見分けられないので、○の有無で
            Some(l) => Some(if l == "x" {
                Mode::X
            } else if shapes::bankara_open(work) {
                Mode::BankaraOpen
            } else {
                Mode::BankaraChallenge
            }),
            None => shapes::matching_mode(work)
                .inspect(|m| notes.shape(work, p("matching"), if *m == Mode::X { "x" } else { "bankara" })),
        };
        if let Some(m) = matching {
            // 左のパネルに自分の値も出ている（右上は「対戦相手を待っています」で、メニューの値は出ない）
            let (id, short) = if m == Mode::X {
                ("matching_x_value", "マッチングの X パワー")
            } else {
                ("matching_udemae_value", "マッチングのウデマエ")
            };
            let n = self.read_glyphs(work, p(id));
            notes.number(p(id), short, &n);
            let what = if m == Mode::X {
                parse_power(&n.text).map(|value| Observed::X { rule: None, value: Some(value) })
            } else {
                parse_points(&n.text).map(|value| Observed::Udemae { value: Some(value), rank: None })
            };
            if let Some(what) = what {
                return Seen::MatchingValue { mode: m, what, wins: None, losses: None };
            }
            return Seen::Matching(m);
        }

        // 精算の見出しが見本で決まらなくても、ゲージがあれば精算（昇格の画面は見出しの見本が要る）
        let title = self.decide(work, p("udemae_title"), UDEMAE_TITLE_MIN, notes);
        if title.as_deref() == Some("promoted") {
            let n = self.read_glyphs(work, p("udemae_reset"));
            notes.number(p("udemae_reset"), "リセット", &n);
            if let Some(v) = parse_points(&n.text) {
                return Seen::UdemaeReset(v);
            }
        } else if title.is_some() || shapes::udemae_gauge(work) {
            if title.is_none() {
                notes.text.push("形で見分けた: 精算のゲージ".into());
            }
            let n = self.read_glyphs(work, p("udemae_value"));
            notes.number(p("udemae_value"), "ウデマエ", &n);
            // 増減は、ゲージの数字が読めなくても読む（読めない字を計算で埋めるのに使う）。
            // チャレンジは最後に TOTAL、オープンは 1 試合ごとに、少し上の段に出る
            let mode = shapes::udemae_mode(work);
            let id = if mode == Some(Mode::BankaraOpen) { "udemae_delta" } else { "udemae_total" };
            let t = self.read_glyphs(work, p(id));
            notes.number(p(id), if id == "udemae_delta" { "増減" } else { "TOTAL" }, &t);
            let total = parse_points(&t.text).or_else(|| sure_guess(p(id), &t).and_then(|g| parse_points(&g)));
            if let Some(value) = parse_points(&n.text) {
                return Seen::Udemae { value, total, mode };
            }
            return Seen::UdemaeScreen { mode, total };
        }

        // 参加費の確かめ（チャレンジを始めるとき）。数字は参加費を引いた後の値まで数え下がる
        if shapes::entry_fee(work) {
            notes.text.push("形で見分けた: 参加費".into());
            let n = self.read_glyphs(work, p("fee_value"));
            notes.number(p("fee_value"), "参加費の後", &n);
            let f = self.read_glyphs(work, p("fee_amount"));
            notes.number(p("fee_amount"), "参加費", &f);
            let after = parse_points(&n.text).or_else(|| sure_guess(p("fee_value"), &n).and_then(|g| parse_points(&g)));
            return Seen::EntryFee { after, fee: parse_points(&f.text) };
        }

        // ロビーのメニューに出ている自分の値（observed）と、選んでいるモードとルール（lobby）
        let lobby = shapes::menu_selection(work);
        if let Some((m, r)) = lobby {
            notes.text.push(format!("メニュー: {} {}", m.as_str(), r.map_or("（ルールなし）", |r| r.as_str())));
        }
        for menu in [Self::menu_x, Self::menu_udemae] {
            match menu(self, work, notes) {
                Some(Some((mut what, wins, losses))) => {
                    // X パワーはルールごとなので、メニューのルールを付ける
                    if let (Observed::X { rule, .. }, Some((Mode::X, r))) = (&mut what, lobby) {
                        *rule = r;
                    }
                    return Seen::Observed { what, wins, losses, lobby };
                }
                Some(None) => break,
                None => {}
            }
        }
        if let Some((mode, rule)) = lobby {
            return Seen::Lobby { mode, rule };
        }

        // 試合後の進行の画面: 勝ち負けは数字ではなく、○の判子とイカの色で数える（数字は判子の後から変わる）
        if self.decide(work, p("progress_label"), PROGRESS_LABEL_MIN, notes).is_some() {
            if let Some((wins, losses)) = count_progress(work, &PROGRESS_STRIP) {
                notes.text.push(format!("進行: {wins}-{losses}"));
                let mode = shapes::progress_mode(work);
                return Seen::Progress { wins, losses, stamps: Some(wins), mode };
            }
        }

        // X パワーの画面: 「Xパワー」の見出し、または（見本で決まらなければ）真ん中の黒いパネルと、
        // 増減の青緑のしぶきか、決まった場所に「4 桁.1 桁」の数字（手がかりの数字での推測でもよい）
        let label = self.scores(work, p("power_label"));
        let by_label = label.first().is_some_and(|s| s.score >= POWER_LABEL_MIN);
        if let Some(s) = label.first() {
            notes.text.push(format!("「Xパワー」: 一致度 {:.2}", s.score));
            notes.peaks.push(("「Xパワー」".into(), s.label.clone(), s.score));
        }
        let n = self.read_glyphs(work, p("power_number"));
        let by_shape =
            !by_label && shapes::result_panel(work) && (shapes::x_splash(work) || parse_power(&n.guess).is_some());
        if by_label || by_shape {
            if by_shape {
                notes.shape(work, p("power_label"), "x_power");
            }
            notes.number(p("power_number"), "Xパワー", &n);
            // 増減は任意（見本があれば念押しに使う。無ければ旧値から新値へ動いたのを見届けて出す）。
            // 大きな数字が読めなくても読む（読めない字を計算で埋めるのに使う）
            let d = self.read_glyphs(work, p("power_delta"));
            notes.number(p("power_delta"), "増減", &d);
            if let Some(value) = parse_power(&n.text) {
                return Seen::XPower { value, delta: parse_delta(&d.text) };
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
            ("progress_label", "032850", "win_lose"),
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
        assert_eq!(see("042142"), Seen::Header { mode: Mode::BankaraChallenge, rule: Some(Rule::Asari), note: Note::None, medals: shapes::medals(&templates::to_work(&load("042142"))) });
        assert_eq!(see("033345"), Seen::XPower { value: 2194.6, delta: Some(94.6) });
        // 7 の見本が無いので増減は読めない
        assert_eq!(see("101625"), Seen::XPower { value: 2336.8, delta: None });
        assert_eq!(see("132222"), Seen::RuleIntro(Rule::Yagura));
        // バンカラの精算: 見本にしなかった 155（130・685・-15・365 の見本から）と、TOTAL（大きな数字の見本）
        let why = |key: &str| r.recognize(&load(key)).notes.join(" / ");
        let ch = Some(Mode::BankaraChallenge);
        assert_eq!(see("040535"), Seen::Udemae { value: 155, total: Some(25), mode: ch }, "{}", why("040535"));
        assert_eq!(see("040526"), Seen::Udemae { value: 130, total: Some(25), mode: ch }, "{}", why("040526"));
        assert_eq!(see("040905"), Seen::Udemae { value: 365, total: Some(380), mode: ch }, "{}", why("040905"));
        assert_eq!(see("041716"), Seen::UdemaeReset(300), "{}", why("041716"));
        let mm = |s: Seen| match s {
            Seen::MatchingValue { mode, .. } => Seen::Matching(mode),
            s => s,
        };
        assert_eq!(mm(see("032333")), Seen::Matching(Mode::X));
        assert_eq!(
            see("032333"),
            Seen::MatchingValue { mode: Mode::X, what: Observed::X { rule: None, value: Some(2100.0) }, wins: None, losses: None }
        );
        assert_eq!(
            see("040151"),
            Seen::MatchingValue { mode: Mode::BankaraChallenge, what: Observed::Udemae { value: Some(130), rank: None }, wins: None, losses: None }
        );
        // メニューの値（手元の見本では字がそろわないので、見本にした画面を読んで仕組みが通るかだけ確かめる）
        let menu_x = Seen::Observed { what: Observed::X { rule: Some(Rule::Yagura), value: Some(2100.0) }, wins: Some(0), losses: Some(0), lobby: Some((Mode::X, Some(Rule::Yagura))) };
        assert_eq!(see("031924"), menu_x, "{}", why("031924"));
        let menu_ud = Seen::Observed { what: Observed::Udemae { value: Some(1051), rank: Some(crate::rank::Rank { letter: 'S', modifier: 0, num: None }) }, wins: Some(0), losses: Some(0), lobby: Some((Mode::BankaraChallenge, Some(Rule::Asari))) };
        assert_eq!(see("041221"), menu_ud, "{}", why("041221"));
        // 進行の画面（勝ち負けは色で数える。見本にしたのは 032850 の「WIN LOSE」の見出しだけ）
        // 進行の見本のモード（032900 は X、ほかはバンカラのチャレンジ・昇格戦）
        let pr = |key: &str, w, l| {
            let mode = Some(if key == "032900" { Mode::X } else { Mode::BankaraChallenge });
            Seen::Progress { wins: w, losses: l, stamps: Some(w), mode }
        };
        for (key, w, l) in [("032900", 1, 0), ("040351", 0, 1), ("040627", 3, 1), ("040643", 3, 2), ("041437", 2, 1), ("041454", 2, 2)] {
            assert_eq!(see(key), pr(key, w, l), "{key}: {}", why(key));
        }
        assert_eq!(mm(see("040151")), Seen::Matching(Mode::BankaraChallenge));
        // メニュー・順位・試合中（無効試合の札・バトル中・Finish!）・X に挑戦できる・進行
        let quiet = [
            "033416", "042113", "041735", "134014", "134030", "134042", "134056",
        ];
        // 計測中の X のメニュー（値は出ないが、選んでいるモードとルールは読む）
        assert_eq!(see("041804"), Seen::Lobby { mode: Mode::X, rule: Some(Rule::Area) }, "{}", why("041804"));
        // X のセット完了の画面（「3 - 0」と WIN の札が並ぶ）を、進行の画面と取り違えない
        assert!(matches!(see("033100"), Seen::XPower { .. }), "{}", why("033100"));
        for key in quiet {
            assert_eq!(see(key), Seen::Unknown, "{key} は何でもない");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 画像を読んで、見えたものを並べる（`SRW_IMGS=a.jpg;b.png cargo test --release -- --ignored see_imgs --nocapture`）
#[test]
#[ignore]
fn see_imgs() {
    let rec = Recognizer::new(Templates::load(&Templates::default_dir()).unwrap());
    for p in std::env::var("SRW_IMGS").unwrap_or_default().split(';').filter(|p| !p.is_empty()) {
        let r = rec.recognize(&image::open(p).unwrap().to_rgb8());
        println!("{}  {:?}", p.rsplit(['/', '\\']).next().unwrap(), r.seen);
    }
}

/// 2026-10-06 の録画と検出ログから残した見本（バンカラの精算・表彰・メニュー・マッチング。N Air の縁ありと OBS の全画面の両方）を、
/// 決めた読みどおりに読めるか（手元の見本を使う。`cargo test --release -- --include-ignored`）
#[test]
#[ignore]
fn samples_of_20261006() {
    let rec = Recognizer::new(Templates::load(&Templates::default_dir()).unwrap());
    let dir = crate::samples_dir().join("snaps");
    let want = [
        ("20261006-000920", "UdemaeScreen { mode: Some(BankaraOpen), total: Some(-13) }"),
        ("20261006-002816", "Header { mode: BankaraChallenge, rule: Some(Asari), note: None, medals: Some((1, 2)) }"),
        ("20261006-002819", "UdemaeScreen { mode: Some(BankaraChallenge), total: Some(80) }"),
        ("20261006-003837", "Header { mode: BankaraChallenge, rule: Some(Area), note: None, medals: Some((1, 2)) }"),
        ("20261006-003841", "UdemaeScreen { mode: Some(BankaraOpen), total: Some(20) }"),
        ("20261006-005319", "medals: Some((2, 1))"),
        ("20261006-010512", "medals: Some((3, 0))"),
        ("20261006-010515", "Progress { wins: 1, losses: 1, stamps: Some(1), mode: Some(BankaraChallenge) }"),
        ("20261006-013036", "Header { mode: X, rule: Some(Area), note: None, medals: Some((0, 1)) }"),
        ("20261006-043708", "rank: Some(Rank { letter: 'S', modifier: 1, num: Some(1) }) }, wins: Some(1), losses: Some(2), lobby: Some((BankaraChallenge, Some(Area)))"),
        ("20261006-044543", "Matching(BankaraOpen)"),
        ("20261006-044843", "medals: Some((0, 2))"),
        ("20261006-044847_オープンの精算 LOSE", "UdemaeScreen { mode: Some(BankaraOpen), total: Some(-13) }"),
        ("20261006-044905", "lobby: Some((BankaraOpen, Some(Asari)))"),
        ("20261006-045416", "medals: Some((0, 1))"),
        ("20261006-045420", "UdemaeScreen { mode: Some(BankaraOpen), total: Some(-13) }"),
        ("20261006-045455", "Lobby { mode: BankaraChallenge, rule: Some(Area) }"),
        ("20261006-045502", "MatchingValue { mode: BankaraChallenge, what: Udemae { value: Some(258)"),
        ("20261008-012019", "Lobby { mode: Other, rule: None }"),
        ("20261008-012013", "Lobby { mode: Other, rule: Some(TurfWar) }"),
    ];
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
    let mut bad = Vec::new();
    for (key, part) in want {
        let p = files.iter().find(|p| p.file_name().unwrap().to_string_lossy().starts_with(key)).unwrap_or_else(|| panic!("見本 {key} が無い"));
        let seen = format!("{:?}", rec.recognize(&image::open(p).unwrap().to_rgb8()).seen);
        if !seen.contains(part) {
            bad.push(format!("{key}: {seen}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// 攻略サイトのメニューの画像（samples/snaps/web。ファイル名の先頭がランク）のランクを読めるか
#[test]
#[ignore]
fn ranks_of_web_menus() {
    let dir = crate::samples_dir().join("snaps").join("web");
    let mut n = 0;
    for p in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())) {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with(".png") || name.starts_with("街_") {
            continue;
        }
        let want = name.split('_').next().unwrap();
        let work = templates::to_work(&image::open(&p).unwrap().to_rgb8());
        let got = crate::rank::read_menu(&work, shapes::rank_letter, |_| None).map(|r| r.to_string());
        assert_eq!(got.as_deref(), Some(want), "{name}");
        n += 1;
    }
    assert!(n >= 9, "画像が足りない（{n} 枚）");
}
