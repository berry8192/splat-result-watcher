//! 状態の移り変わり（docs/design.md の「状態遷移」「途中の値を拾わない」）。
//!
//! 毎フレーム「何が見えたか」（[`Seen`]）を受け取り、段階を進めて、流す出来事（seq の無い JSON）を返す。
//! 撮影と照合からは切り離してあり、試験で確かめられる。
//!
//! - **バトルを見てから勝敗を数える**: 勝敗の画面はロビーで戦績を開いても映るので、「バトル → 勝敗」の
//!   順に見たときだけ 1 試合にする。バトル中の判定がまだ無いうちは [`Config::without_battle`] で
//!   「勝敗の画面が 30 秒以上映っていなかった」に代える
//! - **勝敗はすぐには出さない**: モードは後から出る個人リザルトの見出しで分かる。モードが X・バンカラと
//!   分かってから `result` を出す（ナワバリなどを数えないため。読めなければ出さない）
//! - **値は落ち着いた最後のものを取る**: 3 フレーム同じなら候補にし、画面が消えるまで上書きする。
//!   計算で確かめられるもの（旧値＋増減）は合った時点で出す。合わない・確かめられないものは出さない

// 照合がまだ「映像があるか」だけなので、使われない画面の種類がある。照合をつないだら外す
#![allow(dead_code)]

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
    /// バトル中の画面（おおまかな特徴だけ。値は読まない）
    Battle,
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
/// 勝敗の画面からモードの見出しまで待つ長さ。過ぎたら読めなかったとして出さない
const READ_TIMEOUT: i64 = 120;
/// 試合後の画面を探す長さ（design.md: 3 分）
const POST_TIMEOUT: i64 = 180;
/// バトル中のまま勝敗が来なければ見失ったとみなす長さ
const BATTLE_TIMEOUT: i64 = 15 * 60;
/// バトルを見ずに勝敗を数えるとき、前に勝敗の画面を見てからこれだけ空いていること
const OUTCOME_GAP: i64 = 30;
/// 勝敗の画面がこれより短く途切れても、同じ一続きとみなす（照合の取りこぼし）
const OUTCOME_BLINK: i64 = 3;

#[derive(Clone, Debug)]
pub struct Config {
    /// バトル中の画面の判定がまだ無いので、バトルを見ずに勝敗を数える（`OUTCOME_GAP` で二重を防ぐ）
    pub without_battle: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { without_battle: true }
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

    /// 見えた値（見えなければ `None`）を足す。落ち着いたら `true`
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
                    return true;
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

/// 進めている試合
#[derive(Clone, Debug)]
struct Match {
    id: String,
    started_at: DateTime<Utc>,
    rule: Option<Rule>,
    mode: Option<Mode>,
}

#[derive(Debug)]
enum State {
    Idle,
    InBattle { m: Match, outcome: Settle<Outcome>, nc: Settle<()> },
    Reading { m: Match, outcome: Option<Outcome>, ended_at: DateTime<Utc>, header: Settle<(Mode, Option<Rule>, Note)> },
    PostMatch(Box<Post>),
}

/// 試合後に探す画面。それぞれ一度だけ出す
#[derive(Debug)]
struct Post {
    m: Match,
    since: DateTime<Utc>,
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

pub struct Machine {
    cfg: Config,
    state: State,
    /// 映像が無いフレームの続いた数（段階の表示だけ。試合の途中なら中身は持ったまま）
    dark: u32,
    /// バトルの判定（ルール紹介・バトル中の画面）
    intro: Settle<Rule>,
    battle: Settle<()>,
    /// バトルを見ずに数えるときの勝敗の画面
    idle_outcome: Settle<Outcome>,
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
            state: State::Idle,
            dark: 0,
            intro: Settle::new(STABLE_INTRO),
            battle: Settle::new(STABLE),
            idle_outcome: Settle::new(STABLE),
            episode: None,
            episode_fresh: false,
            serial: 0,
        }
    }

    pub fn stage(&self) -> Stage {
        if self.dark >= NO_SIGNAL {
            return Stage::NoSignal;
        }
        match self.state {
            State::Idle => Stage::Idle,
            State::InBattle { .. } => Stage::InBattle,
            State::Reading { .. } => Stage::Reading,
            State::PostMatch(_) => Stage::PostMatch,
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

        // バトルの始まり（Idle・試合後のどちらからでも）
        let intro = self.intro.push(match &seen {
            Seen::RuleIntro(r) => Some(*r),
            _ => None,
        });
        let battle = self.battle.push((seen == Seen::Battle).then_some(()));
        let starting = (intro || battle)
            && matches!(self.state, State::Idle | State::PostMatch(_));
        if starting {
            if let State::PostMatch(p) = &mut self.state {
                Self::flush(p, at, &mut out);
            }
            let rule = intro.then_some(self.intro.latest).flatten();
            let m = self.new_match(at, rule);
            let mut ev = json!({"type": "battle_started", "match_id": m.id, "at": time(at)});
            if let Some(r) = rule {
                ev["rule"] = r.as_str().into();
            }
            out.push(ev);
            self.state = State::InBattle { m, outcome: Settle::new(STABLE), nc: Settle::new(STABLE) };
            return out;
        }

        let state = std::mem::replace(&mut self.state, State::Idle);
        self.state = match state {
            State::Idle => self.idle(at, &seen),
            State::InBattle { m, mut outcome, mut nc } => {
                // 始まりのルール紹介が続いている間に、ルールが分かれば持つ
                let mut m = m;
                if m.rule.is_none() {
                    m.rule = self.intro.latest;
                }
                let o = outcome.push(match &seen {
                    Seen::Outcome(o) => Some(*o),
                    _ => None,
                });
                let n = nc.push((seen == Seen::NoContestNotice).then_some(()));
                if o || n {
                    self.episode_fresh = false;
                    State::Reading {
                        m,
                        outcome: if n { None } else { outcome.latest },
                        ended_at: at,
                        header: Settle::new(STABLE),
                    }
                } else if at - m.started_at > Duration::seconds(BATTLE_TIMEOUT) {
                    State::Idle
                } else {
                    State::InBattle { m, outcome, nc }
                }
            }
            State::Reading { m, outcome, ended_at, mut header } => {
                let h = header.push(match &seen {
                    Seen::Header { mode, rule, note } => Some((*mode, *rule, *note)),
                    _ => None,
                });
                if h {
                    let (mode, rule, note) = header.latest.unwrap();
                    self.finish(m, outcome, ended_at, mode, rule, note, at, &mut out)
                } else if at - ended_at > Duration::seconds(READ_TIMEOUT) {
                    // モードが読めなかった。ナワバリなどかもしれないので出さない
                    State::Idle
                } else {
                    State::Reading { m, outcome, ended_at, header }
                }
            }
            State::PostMatch(mut p) => {
                Self::post(&mut p, at, &seen, &mut out);
                if at - p.since > Duration::seconds(POST_TIMEOUT) {
                    Self::flush(&mut p, at, &mut out);
                    State::Idle
                } else {
                    State::PostMatch(p)
                }
            }
        };
        out
    }

    fn new_match(&mut self, at: DateTime<Utc>, rule: Option<Rule>) -> Match {
        self.serial = (self.serial + 1) % 100;
        let id = format!("{}-{:02}", at.with_timezone(&Local).format("%Y%m%d-%H%M"), self.serial);
        Match { id, started_at: at, rule, mode: None }
    }

    /// Idle: バトルを見ずに数えるなら、勝敗の画面が久しぶりに落ち着いたところで試合にする
    fn idle(&mut self, at: DateTime<Utc>, seen: &Seen) -> State {
        if !self.cfg.without_battle {
            return State::Idle;
        }
        let o = match seen {
            // 少し前にも勝敗の画面を見ていたもの（同じ試合の続き・戦績の見返し）は数えない
            Seen::Outcome(o) if self.episode_fresh => Some(*o),
            _ => None,
        };
        if self.idle_outcome.push(o) {
            self.idle_outcome = Settle::new(STABLE);
            self.episode_fresh = false;
            // 始まりは分からないので、勝敗を見た時刻で代える
            let m = self.new_match(at, None);
            return State::Reading { m, outcome: Some(o.unwrap()), ended_at: at, header: Settle::new(STABLE) };
        }
        State::Idle
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        mut m: Match,
        outcome: Option<Outcome>,
        ended_at: DateTime<Utc>,
        mode: Mode,
        rule: Option<Rule>,
        note: Note,
        at: DateTime<Utc>,
        out: &mut Vec<Value>,
    ) -> State {
        if mode == Mode::Other {
            return State::Idle;
        }
        let outcome = match (note, outcome) {
            (Note::NoContest, _) | (_, None) => "no_contest",
            (Note::Uncounted, Some(Outcome::Lose)) => "lose_uncounted",
            (_, Some(Outcome::Win)) => "win",
            (_, Some(Outcome::Lose)) => "lose",
        };
        m.mode = Some(mode);
        if rule.is_some() {
            m.rule = rule;
        }
        let mut ev = json!({
            "type": "result", "match_id": m.id, "outcome": outcome, "mode": mode.as_str(),
            "started_at": time(m.started_at), "ended_at": time(ended_at),
        });
        if let Some(r) = m.rule {
            ev["rule"] = r.as_str().into();
        }
        out.push(ev);
        if outcome == "no_contest" {
            return State::Idle;
        }
        State::PostMatch(Box::new(Post {
            m,
            since: at,
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
        }))
    }

    fn post(p: &mut Post, at: DateTime<Utc>, seen: &Seen, out: &mut Vec<Value>) {
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
                    Self::power_x(p, at, Some(before), after, out);
                }
            }
        }

        if *seen == Seen::Calibrating && !p.calibrating_done {
            p.calibrating_done = true;
            out.push(json!({"type": "power", "match_id": p.m.id, "kind": "x", "rule": p.m.rule.map(Rule::as_str),
                "calibrating": true, "at": time(at)}));
        }
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
                    Self::power_udemae(p, at, Some(before), after, out);
                }
            }
        }

        // 進行: WIN の判子の数と勝ち数が合うときだけ数える（数字は判子の後から変わる）
        p.progress.push(match seen {
            Seen::Progress { wins, losses, stamps } if stamps.is_none_or(|s| s == *wins) => Some((*wins, *losses)),
            _ => None,
        });

        Self::gone(p, at, out);
    }

    /// 画面が消えたものを出す。確かめの材料が無いものは、値が動いたのを見届けたときだけ
    fn gone(p: &mut Post, at: DateTime<Utc>, out: &mut Vec<Value>) {
        if !p.xp_done && p.xp.gone() {
            let (first, last) = (p.xp.first.unwrap(), p.xp.latest.unwrap());
            let delta_ok = p.xp_delta.is_none_or(|d| first + d == last);
            if first != last && delta_ok || p.xp_delta == Some(0) {
                Self::power_x(p, at, Some(first), last, out);
            } else {
                // 動いたところを見ていない・増減と合わない。どれが新しい値か分からないので出さない
                p.xp_done = true;
            }
        }
        if !p.calibrated_done && p.calibrated.gone() {
            p.calibrated_done = true;
            out.push(json!({"type": "power", "match_id": p.m.id, "kind": "x", "rule": p.m.rule.map(Rule::as_str),
                "before": null, "after": p.calibrated.latest.unwrap() as f64 / 10.0,
                "calibrating": false, "at": time(at)}));
        }
        if !p.udemae_done && p.udemae.gone() {
            let (first, last) = (p.udemae.first.unwrap(), p.udemae.latest.unwrap());
            let total_ok = p.udemae_total.is_none_or(|t| first + t == last);
            if first != last && total_ok || p.udemae_total == Some(0) {
                Self::power_udemae(p, at, Some(first), last, out);
            } else {
                p.udemae_done = true;
            }
        }
        if !p.progress_done && p.progress.gone() {
            p.progress_done = true;
            let (w, l) = p.progress.latest.unwrap();
            out.push(json!({"type": "set_progress", "match_id": p.m.id, "wins": w, "losses": l}));
        }
    }

    /// 試合後を抜けるとき、見えていた画面は消えたものとして扱う
    fn flush(p: &mut Post, at: DateTime<Utc>, out: &mut Vec<Value>) {
        p.xp.absent = p.xp.absent.max(GONE);
        p.calibrated.absent = p.calibrated.absent.max(GONE);
        p.udemae.absent = p.udemae.absent.max(GONE);
        p.progress.absent = p.progress.absent.max(GONE);
        Self::gone(p, at, out);
    }

    fn power_x(p: &mut Post, at: DateTime<Utc>, before: Option<i64>, after: i64, out: &mut Vec<Value>) {
        p.xp_done = true;
        out.push(json!({"type": "power", "match_id": p.m.id, "kind": "x", "rule": p.m.rule.map(Rule::as_str),
            "before": before.map(|v| v as f64 / 10.0), "after": after as f64 / 10.0,
            "calibrating": false, "at": time(at)}));
    }

    fn power_udemae(p: &mut Post, at: DateTime<Utc>, before: Option<i32>, after: i32, out: &mut Vec<Value>) {
        p.udemae_done = true;
        out.push(json!({"type": "power", "match_id": p.m.id, "kind": "udemae",
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
        fn new(cfg: Config) -> Self {
            Run { m: Machine::new(cfg), i: 0, events: Vec::new() }
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

    #[test]
    fn x_match_from_intro_to_power() {
        let mut r = Run::new(Config::default());
        r.feed(Seen::RuleIntro(Rule::Hoko), 4).feed(Seen::Battle, 20);
        assert_eq!(r.m.stage(), Stage::InBattle);
        r.feed(Seen::Outcome(Outcome::Win), 6).wait(5);
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
        assert_eq!(p["kind"], "x");
        assert_eq!(p["rule"], "hoko");
    }

    #[test]
    fn turf_war_and_unreadable_mode_are_not_counted() {
        let mut r = Run::new(Config::default());
        r.feed(Seen::Battle, 10).feed(Seen::Outcome(Outcome::Win), 6).wait(10);
        r.feed(header(Mode::Other, Rule::Area, Note::None), 5);
        assert_eq!(r.types(), ["battle_started"]);
        assert_eq!(r.m.stage(), Stage::Idle);

        let mut r = Run::new(Config::default());
        r.feed(Seen::Battle, 10).feed(Seen::Outcome(Outcome::Win), 6).wait(READ_TIMEOUT + 5);
        assert_eq!(r.types(), ["battle_started"]);
        assert_eq!(r.m.stage(), Stage::Idle);
    }

    #[test]
    fn lose_uncounted_and_no_contest() {
        let mut r = Run::new(Config::default());
        r.feed(Seen::Battle, 10).feed(Seen::Outcome(Outcome::Lose), 6).wait(10);
        r.feed(header(Mode::X, Rule::Area, Note::Uncounted), 5);
        assert_eq!(r.of("result")[0]["outcome"], "lose_uncounted");

        let mut r = Run::new(Config::default());
        r.feed(Seen::Battle, 10).feed(Seen::NoContestNotice, 4).wait(10);
        r.feed(header(Mode::BankaraChallenge, Rule::Asari, Note::NoContest), 5);
        let res = r.of("result")[0];
        assert_eq!(res["outcome"], "no_contest");
        assert_eq!(res["mode"], "bankara_challenge");
        assert_eq!(r.m.stage(), Stage::Idle);
    }

    #[test]
    fn without_battle_counts_only_a_fresh_outcome_screen() {
        let mut r = Run::new(Config::default());
        r.feed(Seen::Outcome(Outcome::Win), 8).wait(10);
        r.feed(header(Mode::X, Rule::Yagura, Note::None), 5);
        assert_eq!(r.types(), ["result"]);

        // ナワバリだったので Idle に戻った直後、勝敗の画面がまた映った（30 秒以内）。数えない
        let mut r = Run::new(Config::default());
        r.feed(Seen::Outcome(Outcome::Win), 8).feed(header(Mode::Other, Rule::Area, Note::None), 4);
        assert_eq!(r.m.stage(), Stage::Idle);
        r.wait(10).feed(Seen::Outcome(Outcome::Lose), 8);
        assert_eq!(r.m.stage(), Stage::Idle);
        // 30 秒以上空けば新しい試合として読む
        r.wait(31).feed(Seen::Outcome(Outcome::Lose), 8);
        assert_eq!(r.m.stage(), Stage::Reading);
    }

    #[test]
    fn outcome_in_idle_is_ignored_when_battle_is_required() {
        let mut r = Run::new(Config { without_battle: false });
        r.feed(Seen::Outcome(Outcome::Win), 8).wait(10).feed(header(Mode::X, Rule::Yagura, Note::None), 5);
        assert!(r.events.is_empty());
    }

    fn post_match(r: &mut Run) {
        r.feed(Seen::Battle, 10).feed(Seen::Outcome(Outcome::Win), 6).wait(5);
        r.feed(header(Mode::X, Rule::Yagura, Note::None), 5).wait(2);
        r.events.clear();
    }

    fn pr(wins: u8, losses: u8, stamps: Option<u8>) -> Seen {
        Seen::Progress { wins, losses, stamps }
    }

    #[test]
    fn progress_takes_the_last_settled_value() {
        // 判子は 2 個なのに数字はまだ 1-1。数字が変わってから数える
        let mut r = Run::new(Config::default());
        post_match(&mut r);
        r.feed(pr(1, 1, Some(2)), 6).feed(pr(2, 1, Some(2)), 4).wait(2);
        let p = r.of("set_progress");
        assert_eq!(p.len(), 1);
        assert_eq!((p[0]["wins"].as_u64(), p[0]["losses"].as_u64()), (Some(2), Some(1)));

        // 判子が読めなくても、落ち着いた最後の値を取る
        let mut r = Run::new(Config::default());
        post_match(&mut r);
        r.feed(pr(1, 1, None), 6).feed(pr(2, 1, None), 4).wait(2);
        let p = r.of("set_progress");
        assert_eq!((p[0]["wins"].as_u64(), p[0]["losses"].as_u64()), (Some(2), Some(1)));
    }

    #[test]
    fn power_is_sent_only_when_it_can_be_trusted() {
        // 新値しか見ていない（旧値を見逃した）。増減とも確かめられないので出さない
        let mut r = Run::new(Config::default());
        post_match(&mut r);
        r.feed(xp(2194.6, Some(94.6)), 6).wait(2);
        assert!(r.of("power").is_empty());

        // 増減が読めなくても、旧値から動いて落ち着いたのを見届ければ出す
        let mut r = Run::new(Config::default());
        post_match(&mut r);
        r.feed(xp(2100.0, None), 4).feed(xp(2150.0, None), 1).feed(xp(2194.6, None), 4).wait(2);
        let p = r.of("power");
        assert_eq!((p[0]["before"].as_f64(), p[0]["after"].as_f64()), (Some(2100.0), Some(2194.6)));

        // 増減と合わない値で終わったら出さない（読み違い）
        let mut r = Run::new(Config::default());
        post_match(&mut r);
        r.feed(xp(2100.0, Some(94.6)), 4).feed(xp(2191.6, Some(94.6)), 4).wait(2);
        assert!(r.of("power").is_empty());
    }

    #[test]
    fn udemae_and_calibration() {
        let mut r = Run::new(Config::default());
        post_match(&mut r);
        let ud = |v, t| Seen::Udemae { value: v, total: t };
        r.feed(ud(-15, None), 4).feed(ud(-15, Some(380)), 2).feed(ud(120, Some(380)), 1).feed(ud(365, Some(380)), 3);
        let p = r.of("power");
        assert_eq!(p.len(), 1, "TOTAL と合った時点で出す");
        assert_eq!((p[0]["before"].as_i64(), p[0]["after"].as_i64()), (Some(-15), Some(365)));

        let mut r = Run::new(Config::default());
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
        let mut r = Run::new(Config::default());
        post_match(&mut r);
        r.feed(pr(2, 1, Some(2)), 4).feed(Seen::RuleIntro(Rule::Area), 2);
        assert_eq!(r.types(), ["set_progress", "battle_started"]);
        assert_eq!(r.m.stage(), Stage::InBattle);
    }

    #[test]
    fn no_signal_keeps_the_match() {
        let mut r = Run::new(Config::default());
        r.feed(Seen::Battle, 10).feed(Seen::NoSignal, 6);
        assert_eq!(r.m.stage(), Stage::NoSignal);
        r.feed(Seen::Battle, 1);
        assert_eq!(r.m.stage(), Stage::InBattle);
    }
}
