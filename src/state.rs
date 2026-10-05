//! 試合の管理（docs/design.md の「状態遷移」「途中の値を拾わない」）。
//!
//! 毎フレーム「何が見えたか」（[`Seen`]）を受け取り、**今の試合に読めた事実を積み**、流す出来事
//! （seq の無い JSON）を返す。決まった順番には頼らない（見落としがあっても、読めたものから進める）。
//! 撮影と照合からは切り離してあり、試験で確かめられる。
//!
//! - **試合はルール紹介から始まる**: 紹介を見たら、いつでも新しい試合を始める（前の試合はそろった分を出して閉じる）。
//!   紹介を見落としたときは、久しぶり（30 秒以上ぶり）に映った勝敗の画面で始める。
//!   ロビーで戦績を見返したときの勝敗の画面は、久しぶりでなければ数えない
//! - **`result` は勝敗とモードがそろったら出す**: モードは個人リザルトの見出し、マッチング中の画面
//!   （紹介の直前まで見ていたもの）、X パワーの画面（X）、バンカラの精算（チャレンジ）のどれかで分かればよい。
//!   WIN はすぐ出す。LOSE は見出しの「負けとしてカウントされませんでした」を待ち、見出しを見落としたまま
//!   次の試合・時間切れになったら lose で出す。モードが「その他」（ナワバリなど）なら出さない
//! - **値は落ち着いた最後のものを取る**: 3 フレーム同じなら候補にし、画面が消えるまで上書きする。
//!   計算で確かめられるもの（旧値＋増減）は合った時点で出す。合わない・確かめられないものは出さない
//! - 段階（`status`）は積んだ事実から決める: 試合が無い → idle、勝敗がまだ → in_battle、
//!   勝敗はあるがモードがまだ → reading、`result` の後 → post_match

use chrono::{DateTime, Duration, Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rule {
    Area,
    Yagura,
    Hoko,
    Asari,
    /// ナワバリバトル（モードは other。数えないが、試合の区切りとして流す）
    TurfWar,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::Area => "area",
            Rule::Yagura => "yagura",
            Rule::Hoko => "hoko",
            Rule::Asari => "asari",
            Rule::TurfWar => "turf_war",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Win,
    Lose,
}

/// 個人リザルトの見出しの下の文言
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Note {
    None,
    /// 「無効試合になりました」
    NoContest,
    /// 「…負けとしてカウントされませんでした」
    Uncounted,
}

/// メニュー・マッチング中・参加費の画面で見えた自分の値（`observed`）
#[derive(Clone, Copy, Debug, PartialEq)]
/// `value` が `None` なのは、メニューの値の字がまだ読めず、勝ち負けのランプだけ見えたとき
pub enum Observed {
    X { rule: Option<Rule>, value: Option<f64> },
    Udemae { value: Option<i32> },
}

/// 1 フレームで見えたもの。照合が決める
#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    /// 映像が取れない（窓が無い・真っ黒）
    NoSignal,
    /// どれにも当てはまらない
    Unknown,
    /// マッチング中の画面（左のパネルの「Xパワー」「ウデマエ」でモードが分かる）
    Matching(Mode),
    /// マッチング中で、左のパネルの自分の値（X パワー・ウデマエポイント）も読めた。
    /// `Matching` と `Observed` を一度に（勝ち負けの○は読まない）
    MatchingValue { mode: Mode, what: Observed, wins: Option<u8>, losses: Option<u8> },
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
    /// `mode` は見出しと点線の有無で分かるモード（オープンの精算には「挑戦終了!」などの見出しと点線が無い）
    Udemae { value: i32, total: Option<i32>, mode: Option<Mode> },
    /// 精算の画面だが、数字はまだ見本で読めない（モードの手がかりにだけ使う）
    UdemaeScreen { mode: Option<Mode> },
    /// 昇格の画面「300p ウデマエポイントはリセットされます」
    UdemaeReset(i32),
    /// 試合と結び付かない、今見えた自分の値（`wins` / `losses` は進行が見えたときだけ）。
    /// `lobby` はメニューで選んでいるモードとルール（読めたときだけ）
    Observed { what: Observed, wins: Option<u8>, losses: Option<u8>, lobby: Option<(Mode, Rule)> },
    /// ロビーのメニューで選んでいるモードとルール（値の出ないオープン・ナワバリや、値がまだ読めないとき）
    Lobby { mode: Mode, rule: Rule },
    /// 進行の「WIN LOSE n - m」。`stamps` は WIN の判子の数（読めたときだけ）
    /// `mode` はパネルの左上の黄色い札（「チャレンジ」「昇格戦」）で分かるモード（札があればバンカラ、無ければ X）
    Progress { wins: u8, losses: u8, stamps: Option<u8>, mode: Option<Mode> },
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
/// ルール紹介が途切れてまた読めても、最後に見てからこれより短ければ同じ試合
/// （紹介は回線によっては 1 分近く続く。ユーザー情報 2026-10-03）
const INTRO_BLINK: i64 = 10;
/// 紹介を見ずに勝敗の画面で試合を始めるとき、前に勝敗の画面を見てからこれだけ空いていること
const OUTCOME_GAP: i64 = 30;
/// 勝敗の画面がこれより短く途切れても、同じ一続きとみなす（照合の取りこぼし）
const OUTCOME_BLINK: i64 = 3;
/// マッチング中の画面を最後に見てから、これより後に始まった試合にはそのモードを持たせない
/// （マッチングをやめて別のモードに行ったときに持ち越さない）
const MATCHING_FRESH: i64 = 60;

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
/// 落ちても続きから読めるよう、今の試合はファイルに控える（`Machine::save` / `Machine::restore`）。
/// 画面ごとの読みかけ（Settle）は控えない（起動し直したら読み直す）
#[derive(Clone, Debug)]
struct Settle<T> {
    need: u32,
    last: Option<T>,
    run: u32,
    absent: u32,
    first: Option<T>,
    latest: Option<T>,
}

impl<T> Default for Settle<T> {
    fn default() -> Self {
        Settle { need: STABLE, last: None, run: 0, absent: 0, first: None, latest: None }
    }
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
#[derive(Debug, Serialize, Deserialize)]
struct Post {
    #[serde(skip)]
    xp: Settle<i64>,
    /// 読めた X パワーの移り変わり（続けて同じものは 1 つ）。数え上がった後の値は 1 回しか読めないことがある
    #[serde(skip)]
    xp_path: Vec<i64>,
    xp_delta: Option<i64>,
    xp_done: bool,
    calibrating_done: bool,
    #[serde(skip)]
    calibrated: Settle<i64>,
    calibrated_done: bool,
    #[serde(skip)]
    udemae: Settle<i32>,
    udemae_total: Option<i32>,
    udemae_done: bool,
    #[serde(skip)]
    progress: Settle<(u8, u8)>,
    progress_done: bool,
    #[serde(skip)]
    reset: Settle<i32>,
    #[serde(default)]
    reset_done: bool,
}

impl Post {
    fn new() -> Self {
        Post {
            xp: Settle::new(STABLE),
            xp_path: Vec::new(),
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
            reset: Settle::new(STABLE),
            reset_done: false,
        }
    }
}

/// 今の試合と、読めた事実
#[derive(Debug, Serialize, Deserialize)]
struct Game {
    id: String,
    started_at: DateTime<Utc>,
    intro_rule: Option<Rule>,
    outcome: Option<Outcome>,
    no_contest: bool,
    ended_at: Option<DateTime<Utc>>,
    header: Option<(Mode, Option<Rule>, Note)>,
    /// 試合後の画面から分かったモード（X パワー → X、精算 → チャレンジ）
    implied_mode: Option<Mode>,
    /// 紹介の直前のマッチング中の画面で分かったモード
    #[serde(default)]
    matching_mode: Option<Mode>,
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
        // ナワバリの紹介で始まった試合は other（結果の帯が読めなくても）
        let turf = (self.intro_rule == Some(Rule::TurfWar)).then_some(Mode::Other);
        let m = self.header.map(|h| h.0).or(turf).or(self.implied_mode).or(self.matching_mode)?;
        // オープンの精算（見出しと点線が無い）を見たならオープン
        if m == Mode::BankaraChallenge && self.implied_mode == Some(Mode::BankaraOpen) {
            return Some(Mode::BankaraOpen);
        }
        // 結果の帯（形で見たとき）・精算・進行では、バンカラのチャレンジとオープンを見分けられない。
        // 直前のマッチングで○が無ければオープン
        if m == Mode::BankaraChallenge && self.matching_mode == Some(Mode::BankaraOpen) {
            return Some(Mode::BankaraOpen);
        }
        Some(m)
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
    /// ルール紹介が映っている一続き（始まり, 最後に見た時刻）と、試合を始めた一続きの始まり
    intro_episode: Option<(DateTime<Utc>, DateTime<Utc>)>,
    intro_used: Option<DateTime<Utc>>,
    outcome: Settle<Outcome>,
    nc: Settle<()>,
    header: Settle<(Mode, Option<Rule>, Note)>,
    matching: Settle<Mode>,
    /// マッチング中の画面で分かったモードと、最後にその画面を見た時刻
    pending_mode: Option<(Mode, DateTime<Utc>)>,
    /// 直前の試合のルールと始めた時刻（メニューの X パワーにルールを補う。同じローテの枠のときだけ）
    last_rule: Option<(Rule, DateTime<Utc>)>,
    /// 今見えた自分の値（比べやすいよう 0.1 刻みの整数にしたもの）と、最後に出したもの
    observed: Settle<ObservedKey>,
    observed_sent: Option<ObservedKey>,
    /// メニューで選んでいるモードとルールと、最後に出したもの（試合を始めると出し直せるよう忘れる）
    lobby: Settle<(Mode, Rule)>,
    lobby_sent: Option<(Mode, Rule)>,
    /// 勝敗の画面が映っている一続き（始まり, 最後に見た時刻）と、それが久しぶりに映ったものか
    episode: Option<(DateTime<Utc>, DateTime<Utc>)>,
    episode_fresh: bool,
    /// 試合 ID の通し番号（同じ分に 2 試合あっても重ならないように）
    serial: u32,
}

/// `observed` を比べる形: (x か, ルール, 値の 10 倍, 勝ち, 負け)
type ObservedKey = (bool, Option<Rule>, Option<i64>, Option<u8>, Option<u8>);

/// ローテの枠（奇数時から 2 時間）の番号。同じ枠なら同じルール
fn rotation_slot(at: DateTime<Utc>) -> i64 {
    // 奇数時（日本時間）に切り替わる。UTC でも奇数時は奇数時（+9 時間）
    (at.timestamp() - 3600).div_euclid(7200)
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
            intro_episode: None,
            intro_used: None,
            outcome: Settle::new(STABLE),
            nc: Settle::new(STABLE),
            header: Settle::new(STABLE),
            matching: Settle::new(STABLE),
            pending_mode: None,
            last_rule: None,
            observed: Settle::new(STABLE),
            observed_sent: None,
            lobby: Settle::new(STABLE),
            lobby_sent: None,
            episode: None,
            episode_fresh: false,
            serial: 0,
        }
    }

    /// 控えるもの（今の試合と通し番号）。試合が無ければ `null` の試合を控える
    pub fn save(&self) -> String {
        json!({"serial": self.serial, "game": self.game}).to_string()
    }

    /// 控えから読み戻す。時間切れの決まりに当てはまる古い試合は捨てる。読み戻した試合の ID を返す
    pub fn restore(&mut self, saved: &str, now: DateTime<Utc>) -> Option<String> {
        let v: Value = serde_json::from_str(saved).ok()?;
        if let Some(n) = v["serial"].as_u64() {
            self.serial = n as u32;
        }
        let g: Game = serde_json::from_value(v["game"].clone()).ok()?;
        let stale = match (g.has_end(), g.settled) {
            (false, _) => now - g.started_at > Duration::seconds(BATTLE_TIMEOUT),
            (true, false) => now - g.ended_at.unwrap_or(g.last_fact) > Duration::seconds(READ_TIMEOUT),
            (true, true) => now - g.last_fact > Duration::seconds(POST_TIMEOUT),
        };
        if stale {
            return None;
        }
        let id = g.id.clone();
        self.game = Some(g);
        Some(id)
    }

    /// 今の試合を何も出さずに捨てる（手で待機に戻す）。捨てた試合の ID を返す
    pub fn drop_game(&mut self) -> Option<String> {
        self.game.take().map(|g| g.id)
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

        // マッチング中の画面: これからの試合のモード
        let mm = match &seen {
            Seen::Matching(m) | Seen::MatchingValue { mode: m, .. } => Some(*m),
            _ => None,
        };
        if self.matching.push(mm) {
            self.pending_mode = self.matching.latest.map(|m| (m, at));
        }
        if let (Some(m), Some((pm, t))) = (mm, &mut self.pending_mode) {
            if *pm == m {
                *t = at;
            }
        }

        // 今見えた自分の値: 落ち着いて、前に出したものと違えば出す。試合中は出さない
        let slot_rule = self
            .last_rule
            .filter(|(_, t)| rotation_slot(*t) == rotation_slot(at))
            .map(|(r, _)| r);
        let ob = match &seen {
            Seen::Observed { what, wins, losses, .. } | Seen::MatchingValue { what, wins, losses, .. } => Some(match what {
                Observed::X { rule, value } => (true, rule.or(slot_rule), value.map(tenths), *wins, *losses),
                Observed::Udemae { value } => (false, None, value.map(|v| v as i64 * 10), *wins, *losses),
            }),
            _ => None,
        };
        let in_battle = self.game.as_ref().is_some_and(|g| !g.has_end());
        if self.observed.push(ob) && !in_battle && self.observed.latest != self.observed_sent {
            let (x, rule, v, wins, losses) = self.observed.latest.unwrap();
            let mut ev = json!({"type": "observed", "kind": if x { "x" } else { "udemae" }, "at": time(at)});
            if let Some(v) = v {
                ev["value"] = if x { json!(v as f64 / 10.0) } else { json!(v / 10) };
            }
            if let Some(r) = rule {
                ev["rule"] = r.as_str().into();
            }
            if let (Some(w), Some(l)) = (wins, losses) {
                ev["wins"] = w.into();
                ev["losses"] = l.into();
            }
            out.push(ev);
            self.observed_sent = self.observed.latest;
        }

        // メニューで選んでいるモードとルール: 落ち着いて、前に出したものと違えば出す。試合中は出さない
        let lb = match &seen {
            Seen::Lobby { mode, rule } | Seen::Observed { lobby: Some((mode, rule)), .. } => Some((*mode, *rule)),
            _ => None,
        };
        if self.lobby.push(lb) && !in_battle && self.lobby.latest != self.lobby_sent {
            let (mode, rule) = self.lobby.latest.unwrap();
            out.push(json!({"type": "lobby", "mode": mode.as_str(), "rule": rule.as_str(), "at": time(at)}));
            self.lobby_sent = self.lobby.latest;
        }

        // ルール紹介: 新しい試合（ちらついて 2 回落ち着いたものは同じ試合）
        let intro = match &seen {
            Seen::RuleIntro(r) => Some(*r),
            _ => None,
        };
        if intro.is_some() {
            self.intro_episode = match self.intro_episode {
                Some((start, last)) if at - last <= Duration::seconds(INTRO_BLINK) => Some((start, at)),
                _ => Some((at, at)),
            };
        }
        if self.intro.push(intro) {
            let r = self.intro.latest;
            // 同じ一続きの紹介で、もう試合を始めていれば同じ試合（途切れてまた読めただけ）
            let episode = self.intro_episode.map(|e| e.0);
            let same = episode.is_some() && self.intro_used == episode && self.game.is_some();
            self.intro_used = episode;
            if same {
                if let Some(g) = &mut self.game {
                    g.intro_rule = g.intro_rule.or(r);
                }
            } else {
                self.close(at, &mut out);
                if let Some(r) = r {
                    self.last_rule = Some((r, at));
                }
                let mut g = self.open(at, r);
                g.matching_mode = self
                    .pending_mode
                    .take()
                    .filter(|(_, t)| at - *t <= Duration::seconds(MATCHING_FRESH))
                    .map(|(m, _)| m)
                    // マッチングが映らなかった（オープンなど）ときは、メニューで選んでいたモード（ルールが同じときだけ）
                    .or(self.lobby.latest.filter(|(_, lr)| Some(*lr) == r && *lr != Rule::TurfWar).map(|(m, _)| m));
                // 試合の後にメニューへ戻ったら、同じ選択でも出し直す
                self.lobby_sent = None;
                let mut ev = json!({"type": "battle_started", "match_id": g.id, "at": time(at)});
                if r == Some(Rule::TurfWar) {
                    ev["mode"] = Mode::Other.as_str().into();
                } else if let Some(m) = g.matching_mode {
                    ev["mode"] = m.as_str().into();
                }
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
                    let mut g = self.open(at, None);
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

        // 試合後の画面からモードが分かる（勝敗が出た後だけ。試合の始まりの「GO!」をゲージと見たことがある）
        if let Some(g) = self.game.as_mut().filter(|g| g.has_end()) {
            match &seen {
                Seen::XPower { .. } | Seen::Calibrating | Seen::Calibrated(_) => g.implied_mode = Some(Mode::X),
                Seen::Udemae { mode: Some(Mode::BankaraOpen), .. } | Seen::UdemaeScreen { mode: Some(Mode::BankaraOpen) } => {
                    g.implied_mode = Some(Mode::BankaraOpen)
                }
                Seen::Udemae { .. } | Seen::UdemaeScreen { .. } | Seen::UdemaeReset(_) => {
                    g.implied_mode = Some(Mode::BankaraChallenge)
                }
                // 結果の帯を飛ばして進行の画面に移ることがある（2026-10-06 の本番）。進行の札でモードが分かる
                Seen::Progress { mode: Some(m), .. } if g.implied_mode.is_none() => g.implied_mode = Some(*m),
                _ => {}
            }
        }

        let mut close = false;
        if let Some(g) = &mut self.game {
            Self::try_result(g, false, &mut out);
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

    fn open(&mut self, at: DateTime<Utc>, rule: Option<Rule>) -> Game {
        self.serial = (self.serial + 1) % 100;
        let id = format!("{}-{:02}", at.with_timezone(&Local).format("%Y%m%d-%H%M"), self.serial);
        Game {
            id,
            started_at: at,
            intro_rule: rule,
            outcome: None,
            no_contest: false,
            ended_at: None,
            header: None,
            implied_mode: None,
            matching_mode: None,
            settled: false,
            counted: false,
            last_fact: at,
            post: Post::new(),
        }
    }

    /// 今の試合を閉じる。出せるものは出してから（`result` を出せなかった試合は何も出さない）
    fn close(&mut self, at: DateTime<Utc>, out: &mut Vec<Value>) {
        if let Some(mut g) = self.game.take() {
            Self::try_result(&mut g, true, out);
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

    /// 勝敗とモードがそろっていれば `result` を出す。`last` は試合を閉じるとき（見出しを待つのをやめる）
    fn try_result(g: &mut Game, last: bool, out: &mut Vec<Value>) {
        if g.settled {
            return;
        }
        let note = g.header.map_or(Note::None, |h| h.2);
        if !g.has_end() && note != Note::NoContest {
            return;
        }
        let Some(mode) = g.mode() else { return };
        // LOSE は「負けとしてカウントされませんでした」が見出しに付くかもしれないので、見出しを待つ
        // （試合後の画面でモードが分かったなら、見出しはもう過ぎている）
        if g.outcome == Some(Outcome::Lose) && g.header.is_none() && g.implied_mode.is_none() && !last {
            return;
        }
        // バンカラは、チャレンジかオープンかをマッチング（○の有無）か試合後の画面（進行の札・精算の見出し）で確かめてから出す。
        // 結果の帯ではどちらか分からない（本番でオープンの勝ちをチャレンジとして出した。2026-10-06）。来ないまま閉じるならチャレンジ
        // 無効試合は試合後の画面が出ないので待たない
        let no_contest = note == Note::NoContest || g.no_contest;
        if mode == Mode::BankaraChallenge && g.implied_mode.is_none() && g.matching_mode.is_none() && !no_contest && !last {
            return;
        }
        g.settled = true;
        // other（ナワバリなど）も、試合の区切りとして流す（受け手は数えない）
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
        if let Some(v) = xp {
            if p.xp_path.last() != Some(&v) {
                p.xp_path.push(v);
            }
        }
        if !p.xp_done {
            // 増減が読めていれば、動いた後の値は 1 回読めただけでよい（計算が合うので）
            if let (Some(before), Some(&after), Some(d)) = (p.xp.first, p.xp_path.last(), p.xp_delta) {
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
            Seen::Udemae { value, total, .. } => {
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

        // 昇格でウデマエポイントがリセットされた（前の値は出さない）
        let reset = match seen {
            Seen::UdemaeReset(v) => Some(*v),
            _ => None,
        };
        if g.post.reset.push(reset) && !g.post.reset_done {
            g.post.reset_done = true;
            out.push(json!({"type": "power", "match_id": g.id, "kind": "udemae",
                "before": null, "after": g.post.reset.latest, "at": time(at)}));
        }

        // 進行: WIN の判子の数と勝ち数が合うときだけ数える（数字は判子の後から変わる）
        g.post.progress.push(match seen {
            Seen::Progress { wins, losses, stamps, .. } if stamps.is_none_or(|s| s == *wins) => Some((*wins, *losses)),
            _ => None,
        });

        Self::gone(g, at, out);
    }

    /// 画面が消えたものを出す。確かめの材料が無いものは、値が動いたのを見届けたときだけ
    fn gone(g: &mut Game, at: DateTime<Utc>, out: &mut Vec<Value>) {
        let p = &mut g.post;
        if !p.xp_done && p.xp.gone() {
            let first = p.xp.first.unwrap();
            let last = Self::xp_after(p).unwrap_or(p.xp.latest.unwrap());
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

    /// 動いた後の X パワー。落ち着いた値が動いていればそれ。動く前のまま画面が変わったなら、
    /// 数え上がりが見えていれば（動く前から一方向に 2 回以上動いた）最後に読めた値
    /// （数え上がりの後の値は、すぐ「現在の順位」に移って 1 回しか読めないことがある。2026-10-06 の本番）
    fn xp_after(p: &Post) -> Option<i64> {
        let first = p.xp.first?;
        if let Some(l) = p.xp.latest.filter(|&l| l != first) {
            return Some(l);
        }
        let i = p.xp_path.iter().rposition(|&v| v == first)?;
        let tail = &p.xp_path[i + 1..];
        let up = tail.windows(2).all(|w| w[0] < w[1]) && tail.first().is_some_and(|&v| v > first);
        let down = tail.windows(2).all(|w| w[0] > w[1]) && tail.first().is_some_and(|&v| v < first);
        let enough = tail.len() >= 2 || p.xp_delta.is_some();
        (enough && (up || down)).then(|| *tail.last().unwrap())
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
        Seen::Progress { wins, losses, stamps, mode: None }
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
    fn long_intro_with_gaps_is_one_match() {
        // 回線によって紹介は 1 分近く続く。途中で何度か読めなくなっても 1 試合
        let mut r = Run::new();
        for _ in 0..5 {
            r.feed(Seen::RuleIntro(Rule::Area), 16).wait(3);
        }
        assert_eq!(r.types(), ["battle_started"]);
        // 紹介が終わって十分たってからの紹介は次の試合（前の試合の勝敗を見落とした）
        r.wait(60).feed(Seen::RuleIntro(Rule::Hoko), 4);
        assert_eq!(r.types(), ["battle_started", "battle_started"]);
    }

    #[test]
    fn turf_war_and_unreadable_mode_are_not_counted() {
        // other の試合も区切りとして流す（受け手は数えない）
        let mut r = Run::new();
        r.intro(Rule::Area).feed(win(), 6).wait(10);
        r.feed(header(Mode::Other, Rule::Area, Note::None), 5);
        assert_eq!(r.types(), ["battle_started", "result"]);
        assert_eq!(r.of("result")[0]["mode"], "other");

        // ナワバリの紹介で始まった試合は、結果の帯が読めなくても other
        let mut r = Run::new();
        r.intro(Rule::TurfWar).feed(win(), 6).wait(10).wait(READ_TIMEOUT + 5);
        let started = r.of("battle_started")[0];
        assert_eq!((started["mode"].as_str(), started["rule"].as_str()), (Some("other"), Some("turf_war")));
        let res = r.of("result");
        assert_eq!((res[0]["mode"].as_str(), res[0]["rule"].as_str(), res[0]["outcome"].as_str()), (Some("other"), Some("turf_war"), Some("win")));

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
        let x = |r: &Run| r.of("result").into_iter().filter(|e| e["mode"] == "x").count();
        r.wait(10).feed(Seen::Outcome(Outcome::Lose), 8).wait(5);
        r.feed(header(Mode::X, Rule::Area, Note::None), 5);
        assert_eq!(x(&r), 0);
        r.wait(31).feed(Seen::Outcome(Outcome::Lose), 8).feed(header(Mode::X, Rule::Area, Note::None), 5);
        assert_eq!(x(&r), 1);
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
        let ud = |v, t| Seen::Udemae { value: v, total: t, mode: None };
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
    fn restart_in_the_middle_continues_the_match() {
        let mut r = Run::new();
        r.intro(Rule::Hoko).feed(win(), 6).wait(5);
        let saved = r.m.save();
        // 落ちて起動し直した（20 秒後）
        let mut m = Machine::new(Config::default());
        let at = r.at() + Duration::seconds(20);
        assert!(m.restore(&saved, at).is_some());
        assert_eq!(m.stage(), Stage::Reading);
        let mut r2 = Run { m, i: r.i + 40, events: Vec::new() };
        r2.feed(header(Mode::X, Rule::Hoko, Note::None), 5);
        let res: Vec<Value> = r2.of("result").into_iter().cloned().collect();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0]["match_id"], r.of("battle_started")[0]["match_id"]);
        assert_eq!(res[0]["rule"], "hoko");
        // 次の試合の ID は重ならない
        r2.intro(Rule::Area);
        assert_ne!(r2.of("battle_started")[0]["match_id"], res[0]["match_id"]);

        // 古すぎる控えは捨てる
        let mut m = Machine::new(Config::default());
        assert!(m.restore(&saved, at + Duration::seconds(READ_TIMEOUT + 60)).is_none());
        assert_eq!(m.stage(), Stage::Idle);
    }

    #[test]
    fn matching_mode_lets_win_out_early_and_lose_wait_for_the_header() {
        // マッチング中の画面で X と分かっていれば、WIN は見出しを待たずに出す
        let mut r = Run::new();
        r.feed(Seen::Matching(Mode::X), 40).feed(Seen::Unknown, 4).intro(Rule::Area);
        assert_eq!(r.of("battle_started")[0]["mode"], "x");
        r.feed(win(), 4);
        assert_eq!(r.of("result")[0]["mode"], "x");
        assert_eq!(r.of("result")[0]["outcome"], "win");

        // LOSE は見出しを待つ（負けとして数えない文言が付くかもしれない）
        let mut r = Run::new();
        r.feed(Seen::Matching(Mode::BankaraChallenge), 10).intro(Rule::Asari).feed(Seen::Outcome(Outcome::Lose), 6);
        assert!(r.of("result").is_empty());
        r.wait(15).feed(header(Mode::BankaraChallenge, Rule::Asari, Note::Uncounted), 4);
        assert_eq!(r.of("result")[0]["outcome"], "lose_uncounted");

        // 見出しを見落としたまま次の試合が始まったら、マッチングのモードで lose を出す
        let mut r = Run::new();
        r.feed(Seen::Matching(Mode::X), 10).intro(Rule::Area).feed(Seen::Outcome(Outcome::Lose), 6).wait(30);
        r.feed(Seen::RuleIntro(Rule::Hoko), 4);
        assert_eq!(r.types(), ["battle_started", "result", "battle_started"]);
        assert_eq!(r.of("result")[0]["outcome"], "lose");

        // マッチングから時間が空いた（やめて別のモードに行った）ならモードを持ち越さない
        let mut r = Run::new();
        r.feed(Seen::Matching(Mode::X), 10).wait(MATCHING_FRESH + 10).intro(Rule::Area).feed(win(), 4);
        assert!(r.of("battle_started")[0].get("mode").is_none());
        assert!(r.of("result").is_empty());
    }

    #[test]
    fn result_comes_before_power_of_the_same_match() {
        // nicomment は power でセットを締めるので、同じ試合の result は必ず先（nicomment からのお願い 2026-10-03）
        let order = |r: &Run| -> Vec<String> {
            r.events.iter().filter(|e| e["type"] == "result" || e["type"] == "power").map(|e| e["type"].as_str().unwrap().to_string()).collect()
        };
        // (a) セットを決めた LOSE の見出しを見落とし、X パワーの画面を読んだ（マッチングで X と分かっている）
        let mut r = Run::new();
        r.feed(Seen::Matching(Mode::X), 10).intro(Rule::Area).feed(Seen::Outcome(Outcome::Lose), 6).wait(20);
        r.feed(xp(2100.0, Some(-30.0)), 4).feed(xp(2070.0, Some(-30.0)), 4);
        assert_eq!(order(&r), ["result", "power"]);
        assert_eq!(r.of("result")[0]["outcome"], "lose");
        // (b) 見出しが読めず、X パワーの画面でモードを補う
        let mut r = Run::new();
        r.intro(Rule::Area).feed(win(), 6).wait(20).feed(xp(2100.0, Some(94.6)), 4).feed(xp(2194.6, Some(94.6)), 4);
        assert_eq!(order(&r), ["result", "power"]);
        // (c) バンカラの精算でモードを補う
        let mut r = Run::new();
        let ud = |v, t| Seen::Udemae { value: v, total: t, mode: None };
        r.intro(Rule::Asari).feed(Seen::Outcome(Outcome::Lose), 6).wait(20).feed(ud(-15, Some(380)), 4).feed(ud(365, Some(380)), 4);
        assert_eq!(order(&r), ["result", "power"]);
    }

    #[test]
    fn progress_tag_gives_the_mode_when_the_header_is_skipped() {
        let mut r = Run::new();
        r.intro(Rule::Asari).feed(Seen::Outcome(Outcome::Lose), 6).wait(10);
        let p = Seen::Progress { wins: 1, losses: 2, stamps: Some(1), mode: Some(Mode::BankaraChallenge) };
        r.feed(p, 6).wait(60).intro(Rule::Asari);
        let res = r.events.iter().find(|e| e["type"] == "result").expect("負けが流れる");
        assert_eq!((res["mode"].as_str(), res["outcome"].as_str()), (Some("bankara_challenge"), Some("lose")));
        let pr = r.events.iter().find(|e| e["type"] == "set_progress").expect("進行も流れる");
        assert_eq!((pr["wins"].as_u64(), pr["losses"].as_u64()), (Some(1), Some(2)));
    }

    #[test]
    fn bankara_open_is_decided_by_the_udemae_screen_without_a_title() {
        let mut r = Run::new();
        r.intro(Rule::Area).feed(win(), 6).feed(header(Mode::BankaraChallenge, Rule::Area, Note::None), 5).wait(5);
        let open = |v| Seen::Udemae { value: v, total: Some(20), mode: Some(Mode::BankaraOpen) };
        r.feed(open(277), 4).feed(open(297), 4).wait(60).intro(Rule::Area);
        let res = r.events.iter().find(|e| e["type"] == "result").unwrap();
        assert_eq!(res["mode"], "bankara_open");
    }

    #[test]
    fn bankara_open_is_decided_by_the_matching() {
        let mut r = Run::new();
        r.feed(Seen::Matching(Mode::BankaraOpen), 10).intro(Rule::Area).feed(win(), 6);
        r.feed(header(Mode::BankaraChallenge, Rule::Area, Note::None), 5).wait(5);
        let res = r.events.iter().find(|e| e["type"] == "result").unwrap();
        assert_eq!(res["mode"], "bankara_open");
    }

    #[test]
    fn matching_with_the_menu_gives_both_the_mode_and_the_value() {
        let mut r = Run::new();
        let mm = Seen::MatchingValue { mode: Mode::X, what: Observed::X { rule: None, value: Some(2100.0) }, wins: Some(1), losses: Some(0) };
        r.feed(mm, 10).intro(Rule::Area);
        let ob: Vec<&Value> = r.events.iter().filter(|e| e["type"] == "observed").collect();
        assert_eq!(ob.len(), 1);
        assert_eq!((ob[0]["value"].as_f64(), ob[0]["wins"].as_u64()), (Some(2100.0), Some(1)));
        let started = r.events.iter().find(|e| e["type"] == "battle_started").unwrap();
        assert_eq!(started["mode"], "x", "マッチングのモードも使う");
    }

    #[test]
    fn lobby_is_sent_when_the_selection_changes_and_fills_the_mode_of_the_next_battle() {
        let mut r = Run::new();
        let open = Seen::Lobby { mode: Mode::BankaraOpen, rule: Rule::Asari };
        r.feed(open.clone(), 4).feed(Seen::Unknown, 2).feed(open.clone(), 4);
        let lb = r.of("lobby");
        assert_eq!(lb.len(), 1);
        assert_eq!((lb[0]["mode"].as_str(), lb[0]["rule"].as_str()), (Some("bankara_open"), Some("asari")));
        // マッチングが映らなくても、試合はメニューで選んでいたモード
        r.intro(Rule::Asari);
        let b = r.of("battle_started");
        assert_eq!(b[0]["mode"], "bankara_open");
        // 試合の後でメニューに戻れば、同じ選択でも出し直す
        r.feed(win(), 6).wait(30).feed(open, 4);
        assert_eq!(r.of("lobby").len(), 2);
    }

    #[test]
    fn observed_values_are_sent_once_when_they_change() {
        let ox = |v: f64, w: Option<u8>, l: Option<u8>| Seen::Observed {
            what: Observed::X { rule: Some(Rule::Hoko), value: Some(v) },
            wins: w,
            losses: l,
            lobby: None,
        };
        let mut r = Run::new();
        // メニューを開いている間ずっと同じ値 → 1 回だけ
        r.feed(ox(1983.5, Some(1), Some(0)), 20).wait(5).feed(ox(1983.5, Some(1), Some(0)), 10);
        let o = r.of("observed");
        assert_eq!(o.len(), 1);
        assert_eq!((o[0]["kind"].as_str(), o[0]["rule"].as_str(), o[0]["value"].as_f64()), (Some("x"), Some("hoko"), Some(1983.5)));
        assert_eq!((o[0]["wins"].as_u64(), o[0]["losses"].as_u64()), (Some(1), Some(0)));
        assert!(o[0].get("match_id").is_none());
        // 進行が変わったら出す。ウデマエ（参加費の前の値）は整数
        r.feed(ox(1983.5, Some(2), Some(0)), 4);
        r.feed(Seen::Observed { what: Observed::Udemae { value: Some(-40) }, wins: None, losses: None, lobby: None }, 4);
        let o = r.of("observed");
        assert_eq!(o.len(), 3);
        assert_eq!((o[2]["kind"].as_str(), o[2]["value"].as_i64()), (Some("udemae"), Some(-40)));

        // 試合中は出さない
        let mut r = Run::new();
        r.intro(Rule::Area).feed(ox(2000.0, None, None), 6);
        assert!(r.of("observed").is_empty());
    }

    #[test]
    fn menu_x_power_gets_the_rule_of_the_same_rotation() {
        let menu = |v: f64| Seen::Observed { what: Observed::X { rule: None, value: Some(v) }, wins: None, losses: None, lobby: None };
        // 12:00 UTC（21:00 日本時間）の枠: 11:00〜13:00 UTC。ヤグラの試合の後のメニュー
        let mut r = Run::new();
        r.intro(Rule::Yagura).feed(win(), 6).wait(200);
        r.feed(menu(2100.0), 4);
        let o = r.of("observed");
        assert_eq!(o[0]["rule"], "yagura");
        // 枠が変わったら付けない（13:00 UTC を越える）
        r.wait(3600).feed(menu(2150.0), 4);
        let o = r.of("observed");
        assert!(o[1].get("rule").is_none(), "{:?}", o[1]);
    }

    #[test]
    fn promotion_resets_udemae() {
        let mut r = Run::new();
        r.intro(Rule::Asari).feed(win(), 6).wait(5).feed(header(Mode::BankaraChallenge, Rule::Asari, Note::None), 4);
        r.events.clear();
        r.feed(Seen::UdemaeReset(300), 6);
        let p = r.of("power");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["kind"], "udemae");
        assert!(p[0]["before"].is_null());
        assert_eq!(p[0]["after"].as_i64(), Some(300));
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

/// 録画（samples/record/日時/HHMMSS_mmm.jpg）を、本番と同じ読み取りと状態の移り変わりに通して、流れる出来事を出す。
/// 照合には手元の見本を使う（`SRW_REC=samples/record/20261006-000342 cargo test --release -- --ignored replay_record --nocapture`）
#[cfg(test)]
mod replay {
    use super::*;
    use crate::recognize::Recognizer;
    use crate::templates::Templates;

    #[test]
    #[ignore]
    fn replay_record() {
        let Ok(dir) = std::env::var("SRW_REC") else { return };
        let dir = std::path::PathBuf::from(dir);
        let day = dir.file_name().unwrap().to_string_lossy()[..8].to_string();
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "jpg")).collect();
        files.sort();
        let rec = Recognizer::new(Templates::load(&Templates::default_dir()).unwrap());
        let mut m = Machine::new(Config::default());
        let mut last = String::new();
        for p in files {
            let stem = p.file_stem().unwrap().to_string_lossy().to_string();
            let local = chrono::NaiveDateTime::parse_from_str(&format!("{day}{}", &stem[..10]), "%Y%m%d%H%M%S_%3f").unwrap();
            let at = local.and_local_timezone(chrono::Local).unwrap().with_timezone(&Utc);
            let img = image::open(&p).unwrap().to_rgb8();
            let seen = rec.recognize(&img).seen;
            let s = format!("{seen:?}");
            let kind = s.split(['(', ' ']).next().unwrap().to_string();
            let verbose = std::env::var("SRW_FROM").is_ok_and(|f| stem.as_str() >= f.as_str())
                && std::env::var("SRW_TO").is_ok_and(|t| stem.as_str() <= t.as_str());
            if verbose || (kind != last && kind != "Unknown") {
                println!("{stem} {}", s.chars().take(90).collect::<String>());
            }
            last = kind;
            for ev in m.feed(at, seen) {
                println!("  → {ev}");
            }
        }
    }
}
