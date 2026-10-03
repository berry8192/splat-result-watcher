//! 試合の管理（docs/design.md の「状態遷移」「途中の値を拾わない」）。
//!
//! 毎フレーム「何が見えたか」（[`Seen`]）を受け取り、**今の試合に読めた事実を積み**、流す出来事
//! （seq の無い JSON）を返す。決まった順番には頼らない（見落としがあっても、読めたものから進める）。
//! 撮影と照合からは切り離してあり、試験で確かめられる。
//!
//! - **試合はルール紹介から始まる**: 紹介を見たら、いつでも新しい試合を始める（前の試合はそろった分を出して閉じる）。
//!   紹介を見落としたときは、久しぶり（30 秒以上ぶり）に映った勝敗の画面で始める。
//!   ロビーで戦績を見返したときの勝敗の画面は、久しぶりでなければ数えない
//! - **`result` は勝敗とモードがそろったら出す**: モードは個人リザルトの見出しで読む。X パワーの画面が
//!   読めたら X、バンカラの精算が読めたらチャレンジと分かるので、見出しを見落としても出せる。
//!   モードが「その他」（ナワバリなど）なら出さない
//! - **値は落ち着いた最後のものを取る**: 3 フレーム同じなら候補にし、画面が消えるまで上書きする。
//!   計算で確かめられるもの（旧値＋増減）は合った時点で出す。合わない・確かめられないものは出さない
//! - 段階（`status`）は積んだ事実から決める: 試合が無い → idle、勝敗がまだ → in_battle、
//!   勝敗はあるがモードがまだ → reading、`result` の後 → post_match

use chrono::{DateTime, Duration, Local, SecondsFormat, Utc};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Area,
    Yagura,
    Hoko,
    Asari,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::Area => "area",
            Rule::Yagura => "yagura",
            Rule::Hoko => "hoko",
            Rule::Asari => "asari",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    X,
    BankaraChallenge,
    BankaraOpen,
    /// ナワバリ・イベント・プラベ・フェスなど。数えない
    Other,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::X => "x",
            Mode::BankaraChallenge => "bankara_challenge",
            Mode::BankaraOpen => "bankara_open",
            Mode::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Win,
    Lose,
}

/// 個人リザルトの見出しの下の文言
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    None,
    /// 「無効試合になりました」
    NoContest,
    /// 「…負けとしてカウントされませんでした」
    Uncounted,
}

/// 1 フレームで見えたもの。照合が決める
#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    /// 映像が取れない（窓が無い・真っ黒）
    NoSignal,
    /// どれにも当てはまらない
    Unknown,
    /// 試合の始まりの「ルール ガチ〇〇」
    RuleIntro(Rule),
    /// 試合中に出る「…無効試合になりました」の札
    NoContestNotice,
    /// 結果発表の左上の「WIN!」「LOSE...」
    Outcome(Outcome),
    /// 個人リザルト（またはスコアボード）の見出し
    Header { mode: Mode, rule: Option<Rule>, note: Note },
    /// X のセット完了の画面。`value` は数え上がる大きな数字、`delta` はしぶきの増減（読めたときだけ）
    XPower { value: f64, delta: Option<f64> },
    /// 「Xパワー 計測中... n/5」
    Calibrating,
    /// 「計測完了!! 1830.4」
    Calibrated(f64),
    /// バンカラの精算。`value` はゲージの下の今のポイント、`total` は「TOTAL = n p」（読めたときだけ）
    Udemae { value: i32, total: Option<i32> },
    /// 進行の「WIN LOSE n - m」。`stamps` は WIN の判子の数（読めたときだけ）
    Progress { wins: u8, losses: u8, stamps: Option<u8> },
}

/// `status` で流す段階
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    NoSignal,
    Idle,
    InBattle,
    Reading,
    PostMatch,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::NoSignal => "no_signal",
            Stage::Idle => "idle",
            Stage::InBattle => "in_battle",
            Stage::Reading => "reading",
            Stage::PostMatch => "post_match",
        }
    }
}

/// 何フレーム同じなら落ち着いたとみなすか（0.5 秒ごとなので 1.5 秒）
const STABLE: u32 = 3;
/// ルール紹介は短いので 2 フレームでよい
const STABLE_INTRO: u32 = 2;
/// 何フレーム見えなければ画面が消えたとみなすか
const GONE: u32 = 2;
/// 何フレーム映像が無ければ no_signal にするか
const NO_SIGNAL: u32 = 4;
/// 勝敗が出ないまま、試合を見失ったとみなす長さ（延長を含めても十分長く）
const BATTLE_TIMEOUT: i64 = 15 * 60;
/// 勝敗からモードが分かるまで待つ長さ。過ぎたら読めなかったとして出さずに閉じる
const READ_TIMEOUT: i64 = 120;
/// `result` の後、最後に何か読めてから試合を閉じるまでの長さ（design.md: 3 分）
const POST_TIMEOUT: i64 = 180;
/// ルール紹介がちらついて 2 回落ち着いても、これより短い間なら同じ試合
const INTRO_SAME: i64 = 30;
/// 紹介を見ずに勝敗の画面で試合を始めるとき、前に勝敗の画面を見てからこれだけ空いていること
const OUTCOME_GAP: i64 = 30;
/// 勝敗の画面がこれより短く途切れても、同じ一続きとみなす（照合の取りこぼし）
const OUTCOME_BLINK: i64 = 3;

#[derive(Clone, Debug)]
pub struct Config {
    /// ルール紹介を見落としたとき、久しぶりに映った勝敗の画面で試合を始める
    pub outcome_without_intro: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { outcome_without_intro: true }
    }
}

/// 値が何フレーム続いたかを数え、落ち着いた最初と最後の値を持つ
#[derive(Clone, Debug)]
struct Settle<T> {
    need: u32,
    last: Option<T>,
    run: u32,
    absent: u32,
    first: Option<T>,
    latest: Option<T>,
}

impl<T: Clone + PartialEq> Settle<T> {
    fn new(need: u32) -> Self {
        Settle { need, last: None, run: 0, absent: 0, first: None, latest: None }
    }

    /// 見えた値（見えなければ `None`）を足す。ちょうど落ち着いたフレームで `true`（続いている間は 1 回だけ）
    fn push(&mut self, v: Option<T>) -> bool {
        match v {
            Some(v) => {
                self.absent = 0;
                if self.last.as_ref() == Some(&v) {
                    self.run += 1;
                } else {
                    self.last = Some(v);
                    self.run = 1;
                }
                if self.run >= self.need {
                    let v = self.last.clone();
                    if self.first.is_none() {
                        self.first = v.clone();
                    }
                    self.latest = v;
                    return self.run == self.need;
                }
            }
            None => {
                self.absent += 1;
                self.last = None;
                self.run = 0;
            }
        }
        false
    }

    /// 落ち着いた値があり、その画面が消えた
    fn gone(&self) -> bool {
        self.latest.is_some() && self.absent >= GONE
    }
}

/// 試合後の画面から読むもの。それぞれ一度だけ出す
#[derive(Debug)]
struct Post {
    xp: Settle<i64>,
    xp_delta: Option<i64>,
    xp_done: bool,
    calibrating_done: bool,
    calibrated: Settle<i64>,
    calibrated_done: bool,
    udemae: Settle<i32>,
    udemae_total: Option<i32>,
    udemae_done: bool,
    progress: Settle<(u8, u8)>,
    progress_done: bool,
}

impl Post {
    fn new() -> Self {
        Post {
            xp: Settle::new(STABLE),
            xp_delta: None,
            xp_done: false,
            calibrating_done: false,
            calibrated: Settle::new(STABLE),
            calibrated_done: false,
            udemae: Settle::new(STABLE),
            udemae_total: None,
            udemae_done: false,
            progress: Settle::new(STABLE),
            progress_done: false,
        }
    }
}

/// 今の試合と、読めた事実
#[derive(Debug)]
struct Game {
    id: String,
    started_at: DateTime<Utc>,
    /// ルール紹介で始めたか（紹介を見落として勝敗の画面で始めたなら false）
    by_intro: bool,
    intro_rule: Option<Rule>,
    outcome: Option<Outcome>,
    no_contest: bool,
    ended_at: Option<DateTime<Utc>>,
    header: Option<(Mode, Option<Rule>, Note)>,
    /// 試合後の画面から分かったモード（X パワー → X、精算 → チャレンジ）
    implied_mode: Option<Mode>,
    /// `result` を出した（その他のモードで出さないと決めたときも true）
    settled: bool,
    counted: bool,
    /// 最後に何か読めた時刻
    last_fact: DateTime<Utc>,
    post: Post,
}

impl Game {
    fn has_end(&self) -> bool {
        self.outcome.is_some() || self.no_contest
    }

    fn mode(&self) -> Option<Mode> {
        self.header.map(|h| h.0).or(self.implied_mode)
    }

    fn rule(&self) -> Option<Rule> {
        self.header.and_then(|h| h.1).or(self.intro_rule)
    }
}

pub struct Machine {
    cfg: Config,
    game: Option<Game>,
    /// 映像が無いフレームの続いた数（段階の表示だけ。試合は持ったまま）
    dark: u32,
    intro: Settle<Rule>,
    outcome: Settle<Outcome>,
    nc: Settle<()>,
    header: Settle<(Mode, Option<Rule>, Note)>,
    /// 勝敗の画面が映っている一続き（始まり, 最後に見た時刻）と、それが久しぶりに映ったものか
    episode: Option<(DateTime<Utc>, DateTime<Utc>)>,
    episode_fresh: bool,
    /// 試合 ID の通し番号（同じ分に 2 試合あっても重ならないように）
    serial: u32,
}

/// パワーは 0.1 刻みなので 10 倍の整数で比べる
fn tenths(v: f64) -> i64 {
    (v * 10.0).round() as i64
}

fn time(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl Machine {
    pub fn new(cfg: Config) -> Self {
        Machine {
            cfg,
            game: None,
            dark: 0,
            intro: Settle::new(STABLE_INTRO),
            outcome: Settle::new(STABLE),
            nc: Settle::new(STABLE),
            header: Settle::new(STABLE),
            episode: None,
            episode_fresh: false,
            serial: 0,
        }
    }

    pub fn stage(&self) -> Stage {
        if self.dark >= NO_SIGNAL {
            return Stage::NoSignal;
        }
        match &self.game {
            None => Stage::Idle,
            Some(g) if !g.has_end() => Stage::InBattle,
            Some(g) if !g.settled => Stage::Reading,
            Some(_) => Stage::PostMatch,
        }
    }

    /// 1 フレームぶん進め、流す出来事を返す
    pub fn feed(&mut self, at: DateTime<Utc>, seen: Seen) -> Vec<Value> {
        let mut out = Vec::new();
        if seen == Seen::NoSignal {
            self.dark += 1;
        } else {
            self.dark = 0;
        }
        if matches!(seen, Seen::Outcome(_)) {
            self.episode = match self.episode {
                Some((start, last)) if at - last <= Duration::seconds(OUTCOME_BLINK) => Some((start, at)),
                prev => {
                    self.episode_fresh = prev.is_none_or(|(_, last)| at - last > Duration::seconds(OUTCOME_GAP));
                    Some((at, at))
                }
            };
        }
        if let Some(g) = &mut self.game {
            if !matches!(seen, Seen::Unknown | Seen::NoSignal) {
                g.last_fact = at;
            }
        }

        // ルール紹介: 新しい試合（ちらついて 2 回落ち着いたものは同じ試合）
        let intro = match &seen {
            Seen::RuleIntro(r) => Some(*r),
            _ => None,
        };
        if self.intro.push(intro) {
            let r = self.intro.latest;
            let same = self.game.as_ref().is_some_and(|g| {
                g.by_intro && !g.has_end() && at - g.started_at <= Duration::seconds(INTRO_SAME)
            });
            if same {
                if let Some(g) = &mut self.game {
                    g.intro_rule = g.intro_rule.or(r);
                }
            } else {
                self.close(at, &mut out);
                let g = self.open(at, true, r);
                let mut ev = json!({"type": "battle_started", "match_id": g.id, "at": time(at)});
                if let Some(r) = r {
                    ev["rule"] = r.as_str().into();
                }
                out.push(ev);
                self.game = Some(g);
            }
        }

        // 勝敗の画面
        let o = match &seen {
            Seen::Outcome(o) => Some(*o),
            _ => None,
        };
        if self.outcome.push(o) {
            let o = self.outcome.latest;
            match &mut self.game {
                Some(g) if !g.has_end() => {
                    g.outcome = o;
                    g.ended_at = Some(at);
                    self.episode_fresh = false;
                }
                // 試合が無い・この試合の勝敗はもう読んだ: 紹介を見落とした新しい試合かもしれない
                _ if self.cfg.outcome_without_intro && self.episode_fresh => {
                    self.close(at, &mut out);
                    let mut g = self.open(at, false, None);
                    g.outcome = o;
                    g.ended_at = Some(at);
                    self.game = Some(g);
                    self.episode_fresh = false;
                }
                _ => {}
            }
        }

        // 試合中の「無効試合になりました」の札
        if self.nc.push((seen == Seen::NoContestNotice).then_some(())) {
            if let Some(g) = &mut self.game {
                if !g.has_end() {
                    g.no_contest = true;
                    g.ended_at = Some(at);
                }
            }
        }

        // 個人リザルトの見出し（試合が無いとき＝ロビーでの見返しは使わない）
        let h = match &seen {
            Seen::Header { mode, rule, note } => Some((*mode, *rule, *note)),
            _ => None,
        };
        if self.header.push(h) {
            if let Some(g) = &mut self.game {
                if g.header.is_none() {
                    g.header = self.header.latest;
                }
            }
        }

        // 試合後の画面からモードが分かる
        if let Some(g) = &mut self.game {
            match &seen {
                Seen::XPower { .. } | Seen::Calibrating | Seen::Calibrated(_) => g.implied_mode = Some(Mode::X),
                Seen::Udemae { .. } => g.implied_mode = Some(Mode::BankaraChallenge),
                _ => {}
            }
        }

        let mut close = false;
        if let Some(g) = &mut self.game {
            Self::try_result(g, &mut out);
            if g.settled && g.counted {
                Self::post(g, at, &seen, &mut out);
            }
            // 見失った試合を閉じる
            let quiet = at - g.last_fact;
            close = match (g.has_end(), g.settled) {
                (false, _) => at - g.started_at > Duration::seconds(BATTLE_TIMEOUT),
                (true, false) => at - g.ended_at.unwrap_or(at) > Duration::seconds(READ_TIMEOUT),
                (true, true) => quiet > Duration::seconds(POST_TIMEOUT),
            };
        }
        if close {
            self.close(at, &mut out);
        }
        out
    }

    fn open(&mut self, at: DateTime<Utc>, by_intro: bool, rule: Option<Rule>) -> Game {
        self.serial = (self.serial + 1) % 100;
        let id = format!("{}-{:02}", at.with_timezone(&Local).format("%Y%m%d-%H%M"), self.serial);
        Game {
            id,
            started_at: at,
            by_intro,
            intro_rule: rule,
            outcome: None,
            no_contest: false,
            ended_at: None,
            header: None,
            implied_mode: None,
            settled: false,
            counted: false,
            last_fact: at,
            post: Post::new(),
        }
    }

    /// 今の試合を閉じる。出せるものは出してから（`result` を出せなかった試合は何も出さない）
    fn close(&mut self, at: DateTime<Utc>, out: &mut Vec<Value>) {
        if let Some(mut g) = self.game.take() {
            Self::try_result(&mut g, out);
            if g.settled && g.counted {
                let p = &mut g.post;
                p.xp.absent = p.xp.absent.max(GONE);
                p.calibrated.absent = p.calibrated.absent.max(GONE);
                p.udemae.absent = p.udemae.absent.max(GONE);
                p.progress.absent = p.progress.absent.max(GONE);
                Self::gone(&mut g, at, out);
            }
        }
    }

    /// 勝敗とモードがそろっていれば `result` を出す
    fn try_result(g: &mut Game, out: &mut Vec<Value>) {
        if g.settled {
            return;
        }
        let note = g.header.map_or(Note::None, |h| h.2);
        if !g.has_end() && note != Note::NoContest {
            return;
        }
        let Some(mode) = g.mode() else { return };
        g.settled = true;
        if mode == Mode::Other {
            return;
        }
        let outcome = match (note, g.no_contest, g.outcome) {
            (Note::NoContest, _, _) | (_, true, _) | (_, _, None) => "no_contest",
            (Note::Uncounted, _, Some(Outcome::Lose)) => "lose_uncounted",
            (_, _, Some(Outcome::Win)) => "win",
            (_, _, Some(Outcome::Lose)) => "lose",
        };
        g.counted = outcome != "no_contest";
        let mut ev = json!({
            "type": "result", "match_id": g.id, "outcome": outcome, "mode": mode.as_str(),
            "started_at": time(g.started_at), "ended_at": time(g.ended_at.unwrap_or(g.last_fact)),
        });
        if let Some(r) = g.rule() {
            ev["rule"] = r.as_str().into();
        }
        out.push(ev);
    }

    fn post(g: &mut Game, at: DateTime<Utc>, seen: &Seen, out: &mut Vec<Value>) {
        let p = &mut g.post;
        // X パワー: 落ち着いた最初の値が旧値。旧値＋増減と合えばその場で出す
        let xp = match seen {
            Seen::XPower { value, delta } => {
                if let Some(d) = delta {
                    p.xp_delta = Some(tenths(*d));
                }
                Some(tenths(*value))
            }
            _ => None,
        };
        p.xp.push(xp);
        if !p.xp_done {
            if let (Some(before), Some(after), Some(d)) = (p.xp.first, p.xp.latest, p.xp_delta) {
                if before + d == after && before != after {
                    Self::power_x(g, at, Some(before), after, out);
                }
            }
        }

        let p = &mut g.post;
        if *seen == Seen::Calibrating && !p.calibrating_done {
            p.calibrating_done = true;
            out.push(json!({"type": "power", "match_id": g.id, "kind": "x", "rule": g.rule().map(Rule::as_str),
                "calibrating": true, "at": time(at)}));
        }
        let p = &mut g.post;
        p.calibrated.push(match seen {
            Seen::Calibrated(v) => Some(tenths(*v)),
            _ => None,
        });

        // ウデマエポイント: 今のポイント＋TOTAL と合えばその場で出す
        let ud = match seen {
            Seen::Udemae { value, total } => {
                if total.is_some() {
                    p.udemae_total = *total;
                }
                Some(*value)
            }
            _ => None,
        };
        p.udemae.push(ud);
        if !p.udemae_done {
            if let (Some(before), Some(after), Some(t)) = (p.udemae.first, p.udemae.latest, p.udemae_total) {
                if before + t == after && before != after {
                    Self::power_udemae(g, at, Some(before), after, out);
                }
            }
        }

        // 進行: WIN の判子の数と勝ち数が合うときだけ数える（数字は判子の後から変わる）
        g.post.progress.push(match seen {
            Seen::Progress { wins, losses, stamps } if stamps.is_none_or(|s| s == *wins) => Some((*wins, *losses)),
            _ => None,
        });

        Self::gone(g, at, out);
    }

    /// 画面が消えたものを出す。確かめの材料が無いものは、値が動いたのを見届けたときだけ
    fn gone(g: &mut Game, at: DateTime<Utc>, out: &mut Vec<Value>) {
        let p = &mut g.post;
        if !p.xp_done && p.xp.gone() {
            let (first, last) = (p.xp.first.unwrap(), p.xp.latest.unwrap());
            let delta_ok = p.xp_delta.is_none_or(|d| first + d == last);
            if first != last && delta_ok || p.xp_delta == Some(0) {
                Self::power_x(g, at, Some(first), last, out);
            } else {
                // 動いたところを見ていない・増減と合わない。どれが新しい値か分からないので出さない
                g.post.xp_done = true;
            }
        }
        if !g.post.calibrated_done && g.post.calibrated.gone() {
            g.post.calibrated_done = true;
            out.push(json!({"type": "power", "match_id": g.id, "kind": "x", "rule": g.rule().map(Rule::as_str),
                "before": null, "after": g.post.calibrated.latest.unwrap() as f64 / 10.0,
                "calibrating": false, "at": time(at)}));
        }
        let p = &mut g.post;
        if !p.udemae_done && p.udemae.gone() {
            let (first, last) = (p.udemae.first.unwrap(), p.udemae.latest.unwrap());
            let total_ok = p.udemae_total.is_none_or(|t| first + t == last);
            if first != last && total_ok || p.udemae_total == Some(0) {
                Self::power_udemae(g, at, Some(first), last, out);
            } else {
                g.post.udemae_done = true;
            }
        }
        let p = &mut g.post;
        if !p.progress_done && p.progress.gone() {
            p.progress_done = true;
            let (w, l) = p.progress.latest.unwrap();
            out.push(json!({"type": "set_progress", "match_id": g.id, "wins": w, "losses": l}));
        }
    }

    fn power_x(g: &mut Game, at: DateTime<Utc>, before: Option<i64>, after: i64, out: &mut Vec<Value>) {
        g.post.xp_done = true;
        out.push(json!({"type": "power", "match_id": g.id, "kind": "x", "rule": g.rule().map(Rule::as_str),
            "before": before.map(|v| v as f64 / 10.0), "after": after as f64 / 10.0,
            "calibrating": false, "at": time(at)}));
    }

    fn power_udemae(g: &mut Game, at: DateTime<Utc>, before: Option<i32>, after: i32, out: &mut Vec<Value>) {
        g.post.udemae_done = true;
        out.push(json!({"type": "power", "match_id": g.id, "kind": "udemae",
            "before": before, "after": after, "at": time(at)}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// 0.5 秒ごとにフレームを流す
    struct Run {
        m: Machine,
        i: i64,
        events: Vec<Value>,
    }

    impl Run {
        fn new() -> Self {
            Run { m: Machine::new(Config::default()), i: 0, events: Vec::new() }
        }
        fn at(&self) -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 10, 3, 12, 0, 0).unwrap() + Duration::milliseconds(500 * self.i)
        }
        fn feed(&mut self, seen: Seen, n: usize) -> &mut Self {
            for _ in 0..n {
                let at = self.at();
                self.events.extend(self.m.feed(at, seen.clone()));
                self.i += 1;
            }
            self
        }
        fn wait(&mut self, secs: i64) -> &mut Self {
            self.feed(Seen::Unknown, (secs * 2) as usize)
        }
        fn intro(&mut self, r: Rule) -> &mut Self {
            self.feed(Seen::RuleIntro(r), 6).wait(60)
        }
        fn types(&self) -> Vec<String> {
            self.events.iter().map(|e| e["type"].as_str().unwrap().to_string()).collect()
        }
        fn of(&self, ty: &str) -> Vec<&Value> {
            self.events.iter().filter(|e| e["type"] == ty).collect()
        }
    }

    fn header(mode: Mode, rule: Rule, note: Note) -> Seen {
        Seen::Header { mode, rule: Some(rule), note }
    }

    fn xp(v: f64, d: Option<f64>) -> Seen {
        Seen::XPower { value: v, delta: d }
    }

    fn win() -> Seen {
        Seen::Outcome(Outcome::Win)
    }

    fn pr(wins: u8, losses: u8, stamps: Option<u8>) -> Seen {
        Seen::Progress { wins, losses, stamps }
    }

    #[test]
    fn x_match_from_intro_to_power() {
        let mut r = Run::new();
        r.intro(Rule::Hoko);
        assert_eq!(r.m.stage(), Stage::InBattle);
        r.feed(win(), 6).wait(5);
        assert_eq!(r.m.stage(), Stage::Reading);
        r.feed(header(Mode::X, Rule::Hoko, Note::None), 5).wait(3);
        assert_eq!(r.m.stage(), Stage::PostMatch);
        // 旧値 → 増減が出る → ドラムロール → 新値
        r.feed(xp(2100.0, None), 4)
            .feed(xp(2100.0, Some(94.6)), 3)
            .feed(xp(2150.3, Some(94.6)), 1)
            .feed(xp(2187.1, Some(94.6)), 1)
            .feed(xp(2194.6, Some(94.6)), 3);
        assert_eq!(r.types(), ["battle_started", "result", "power"]);
        let b = r.of("battle_started")[0];
        assert_eq!(b["rule"], "hoko");
        let res = r.of("result")[0];
        assert_eq!(res["outcome"], "win");
        assert_eq!(res["mode"], "x");
        assert_eq!(res["rule"], "hoko");
        assert_eq!(res["match_id"], b["match_id"]);
        assert_eq!(res["started_at"], b["at"]);
        let p = r.of("power")[0];
        assert_eq!((p["before"].as_f64(), p["after"].as_f64()), (Some(2100.0), Some(2194.6)));
        assert_eq!(p["rule"], "hoko");
        r.wait(POST_TIMEOUT + 1);
        assert_eq!(r.m.stage(), Stage::Idle);
    }

    #[test]
    fn intro_flicker_is_one_match() {
        let mut r = Run::new();
        r.feed(Seen::RuleIntro(Rule::Area), 3).wait(1).feed(Seen::RuleIntro(Rule::Area), 3);
        assert_eq!(r.types(), ["battle_started"]);
    }

    #[test]
    fn turf_war_and_unreadable_mode_are_not_counted() {
        let mut r = Run::new();
        r.intro(Rule::Area).feed(win(), 6).wait(10);
        r.feed(header(Mode::Other, Rule::Area, Note::None), 5);
        assert_eq!(r.types(), ["battle_started"]);

        let mut r = Run::new();
        r.intro(Rule::Area).feed(win(), 6).wait(READ_TIMEOUT + 5);
        assert_eq!(r.types(), ["battle_started"]);
        assert_eq!(r.m.stage(), Stage::Idle);
    }

    #[test]
    fn lose_uncounted_and_no_contest() {
        let mut r = Run::new();
        r.intro(Rule::Area).feed(Seen::Outcome(Outcome::Lose), 6).wait(10);
        r.feed(header(Mode::X, Rule::Area, Note::Uncounted), 5);
        assert_eq!(r.of("result")[0]["outcome"], "lose_uncounted");

        // 試合中の札で終わる
        let mut r = Run::new();
        r.intro(Rule::Asari).feed(Seen::NoContestNotice, 4).wait(10);
        r.feed(header(Mode::BankaraChallenge, Rule::Asari, Note::NoContest), 5);
        let res = r.of("result")[0];
        assert_eq!(res["outcome"], "no_contest");
        assert_eq!(res["mode"], "bankara_challenge");

        // 札を見落としても、見出しの文言で無効試合と分かる
        let mut r = Run::new();
        r.intro(Rule::Asari).feed(header(Mode::BankaraChallenge, Rule::Asari, Note::NoContest), 5);
        assert_eq!(r.of("result")[0]["outcome"], "no_contest");
    }

    #[test]
    fn missed_intro_starts_on_a_fresh_outcome_screen() {
        let mut r = Run::new();
        r.feed(win(), 8).wait(10).feed(header(Mode::X, Rule::Yagura, Note::None), 5);
        assert_eq!(r.types(), ["result"]);
        assert_eq!(r.of("result")[0]["rule"], "yagura");

        // 30 秒以内にまた映った勝敗の画面（戦績の見返し）は数えない。空けば新しい試合
        let mut r = Run::new();
        r.feed(win(), 8).feed(header(Mode::Other, Rule::Area, Note::None), 4);
        r.wait(10).feed(Seen::Outcome(Outcome::Lose), 8).wait(5);
        r.feed(header(Mode::X, Rule::Area, Note::None), 5);
        assert!(r.of("result").is_empty());
        r.wait(31).feed(Seen::Outcome(Outcome::Lose), 8).feed(header(Mode::X, Rule::Area, Note::None), 5);
        assert_eq!(r.of("result").len(), 1);
    }

    #[test]
    fn new_intro_while_reading_starts_the_next_match() {
        // 見出しを読めないまま次の試合が始まった。前の試合は出さずに閉じ、次を始める
        let mut r = Run::new();
        r.intro(Rule::Area).feed(win(), 6).wait(20).intro(Rule::Hoko);
        assert_eq!(r.types(), ["battle_started", "battle_started"]);
        assert_eq!(r.m.stage(), Stage::InBattle);
        assert_eq!(r.of("battle_started")[1]["rule"], "hoko");
    }

    #[test]
    fn x_power_screen_tells_the_mode() {
        // 見出しを見落としたが、X パワーの画面が読めた
        let mut r = Run::new();
        r.intro(Rule::Yagura).feed(win(), 6).wait(20);
        r.feed(xp(2100.0, Some(94.6)), 4).feed(xp(2194.6, Some(94.6)), 4);
        assert_eq!(r.types(), ["battle_started", "result", "power"]);
        assert_eq!(r.of("result")[0]["mode"], "x");
        assert_eq!(r.of("result")[0]["rule"], "yagura");
    }

    fn post_match(r: &mut Run) {
        r.intro(Rule::Yagura).feed(win(), 6).wait(5);
        r.feed(header(Mode::X, Rule::Yagura, Note::None), 5).wait(2);
        r.events.clear();
    }

    #[test]
    fn progress_takes_the_last_settled_value() {
        // 判子は 2 個なのに数字はまだ 1-1。数字が変わってから数える
        let mut r = Run::new();
        post_match(&mut r);
        r.feed(pr(1, 1, Some(2)), 6).feed(pr(2, 1, Some(2)), 4).wait(2);
        let p = r.of("set_progress");
        assert_eq!(p.len(), 1);
        assert_eq!((p[0]["wins"].as_u64(), p[0]["losses"].as_u64()), (Some(2), Some(1)));

        // 判子が読めなくても、落ち着いた最後の値を取る
        let mut r = Run::new();
        post_match(&mut r);
        r.feed(pr(1, 1, None), 6).feed(pr(2, 1, None), 4).wait(2);
        let p = r.of("set_progress");
        assert_eq!((p[0]["wins"].as_u64(), p[0]["losses"].as_u64()), (Some(2), Some(1)));
    }

    #[test]
    fn power_is_sent_only_when_it_can_be_trusted() {
        // 新値しか見ていない（旧値を見逃した）。増減とも確かめられないので出さない
        let mut r = Run::new();
        post_match(&mut r);
        r.feed(xp(2194.6, Some(94.6)), 6).wait(2);
        assert!(r.of("power").is_empty());

        // 増減が読めなくても、旧値から動いて落ち着いたのを見届ければ出す
        let mut r = Run::new();
        post_match(&mut r);
        r.feed(xp(2100.0, None), 4).feed(xp(2150.0, None), 1).feed(xp(2194.6, None), 4).wait(2);
        let p = r.of("power");
        assert_eq!((p[0]["before"].as_f64(), p[0]["after"].as_f64()), (Some(2100.0), Some(2194.6)));

        // 増減と合わない値で終わったら出さない（読み違い）
        let mut r = Run::new();
        post_match(&mut r);
        r.feed(xp(2100.0, Some(94.6)), 4).feed(xp(2191.6, Some(94.6)), 4).wait(2);
        assert!(r.of("power").is_empty());
    }

    #[test]
    fn udemae_and_calibration() {
        let mut r = Run::new();
        post_match(&mut r);
        let ud = |v, t| Seen::Udemae { value: v, total: t };
        r.feed(ud(-15, None), 4).feed(ud(-15, Some(380)), 2).feed(ud(120, Some(380)), 1).feed(ud(365, Some(380)), 3);
        let p = r.of("power");
        assert_eq!(p.len(), 1, "TOTAL と合った時点で出す");
        assert_eq!((p[0]["before"].as_i64(), p[0]["after"].as_i64()), (Some(-15), Some(365)));

        let mut r = Run::new();
        post_match(&mut r);
        r.feed(Seen::Calibrating, 4).wait(2).feed(Seen::Calibrated(1830.4), 4).wait(2);
        let p = r.of("power");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0]["calibrating"], true);
        assert_eq!((p[1]["calibrating"].as_bool(), p[1]["after"].as_f64()), (Some(false), Some(1830.4)));
    }

    #[test]
    fn next_battle_flushes_post_match() {
        // 進行の画面のまま次のバトルが始まった（消えたのを見ていない）
        let mut r = Run::new();
        post_match(&mut r);
        r.feed(pr(2, 1, Some(2)), 4).feed(Seen::RuleIntro(Rule::Area), 2);
        assert_eq!(r.types(), ["set_progress", "battle_started"]);
        assert_eq!(r.m.stage(), Stage::InBattle);
    }

    #[test]
    fn header_in_lobby_without_a_match_is_ignored() {
        let mut r = Run::new();
        r.feed(header(Mode::X, Rule::Area, Note::None), 10);
        assert!(r.events.is_empty());
        assert_eq!(r.m.stage(), Stage::Idle);
    }

    #[test]
    fn no_signal_keeps_the_match() {
        let mut r = Run::new();
        r.intro(Rule::Area).feed(Seen::NoSignal, 6);
        assert_eq!(r.m.stage(), Stage::NoSignal);
        r.feed(Seen::Unknown, 1);
        assert_eq!(r.m.stage(), Stage::InBattle);
    }
}
