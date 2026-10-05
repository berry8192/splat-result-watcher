//! 見本のそろい具合。どの画面を映すと何がそろうか、何が取れていて、何が残っているか（GUI の「そろい具合」）。
//!
//! 見本が無くても形と色（shapes.rs）や手がかりの数字（starter.rs）で読める所は、確かめられたら自動で足される（learn.rs）。
//! 利用者が手で取る必要があるのは、`How::Manual` の所だけ。

use serde::Serialize;

use crate::templates::{glyph_label, is_auto, Kind, Pool, Templates, PLACES};

/// その材料がどうそろうか
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// 見本が無くても形と色で見分ける。別の画面で確かめられたら自動で足す
    Shape,
    /// 手がかりの数字で推測し、「動く前 + 増減 = 動いた後」が合ったら自動で足す
    Sum,
    /// 試合後の値と、その後のメニューの値が同じことで自動で足す
    SameValue,
    /// 手で登録する（見本の登録の画面で）
    Manual,
}

struct Need {
    pool: Pool,
    /// 空なら数字の 0〜9
    labels: &'static [&'static str],
    how: How,
    /// 無いと読めないもの（false は、あると確かになる・付け足しの情報が増えるもの）
    required: bool,
}

struct Screen {
    name: &'static str,
    /// どうすればその画面が映るか
    show: &'static str,
    /// その画面から読めるもの
    gives: &'static str,
    needs: &'static [Need],
}

const DIGITS: &[&str] = &[];

const SCREENS: &[Screen] = &[
    Screen {
        name: "ロビーのメニュー（X マッチ）",
        show: "ロビーでマッチの選択を開き、X マッチにカーソルを合わせる（右上に「Xパワー : 2100.0」と○が出る）",
        gives: "今の X パワーと勝ち負け（observed）",
        needs: &[
            Need { pool: Pool::MenuX, labels: &["x_power"], how: How::Shape, required: false },
            Need { pool: Pool::DigitMenu, labels: DIGITS, how: How::SameValue, required: true },
        ],
    },
    Screen {
        name: "ロビーのメニュー（バンカラ）",
        show: "ロビーでマッチの選択を開き、バンカラマッチ（チャレンジ）にカーソルを合わせる（右上に「ウデマエ」とポイント）",
        gives: "今のウデマエポイントと勝ち負け（observed）",
        needs: &[
            Need { pool: Pool::MenuUdemae, labels: &["udemae"], how: How::Shape, required: false },
            Need { pool: Pool::DigitMenu, labels: DIGITS, how: How::SameValue, required: true },
        ],
    },
    Screen {
        name: "マッチング",
        show: "マッチを始めて、メニューを開いたまま待つ（左のパネルにルール名と「Xパワー 2100.0」か「ウデマエ S130p」）",
        gives: "これから始まる試合のモードと、今の X パワー・ウデマエポイント（observed）",
        needs: &[
            Need { pool: Pool::Matching, labels: &["x", "bankara"], how: How::Shape, required: false },
            // 数字は X パワーの画面と同じ字体（大きな数字の見本を使う）
            Need { pool: Pool::Digit, labels: DIGITS, how: How::SameValue, required: true },
        ],
    },
    Screen {
        name: "ルール紹介",
        show: "試合の始まりに自動で出る（ルールは 4 種）",
        gives: "試合の始まり（battle_started）とルール",
        needs: &[Need { pool: Pool::RuleIntro, labels: &["area", "yagura", "hoko", "asari"], how: How::Shape, required: false }],
    },
    Screen {
        name: "結果発表",
        show: "試合の終わりに自動で出る（勝ちの「WIN!」と負けの「LOSE...」の両方）",
        gives: "勝ち負け（result）",
        needs: &[Need { pool: Pool::Outcome, labels: &["win", "lose"], how: How::Shape, required: false }],
    },
    Screen {
        name: "結果の帯（個人リザルト）",
        show: "結果発表の後、左上に「Xマッチ」などとルール・ステージが出る画面（X とバンカラの両方）",
        gives: "モード（X かバンカラか）とルール",
        needs: &[
            Need { pool: Pool::Mode, labels: &["x", "bankara_challenge"], how: How::Shape, required: false },
            Need { pool: Pool::Rule, labels: &["area", "yagura", "hoko", "asari"], how: How::Manual, required: false },
        ],
    },
    Screen {
        name: "X パワーの変動",
        show: "X マッチを 3 勝か 3 敗で終えた後（X パワーが数え上がり、右に青緑のしぶきで増減）",
        gives: "X パワーの変動（power）",
        needs: &[
            Need { pool: Pool::PowerLabel, labels: &["x_power"], how: How::Shape, required: false },
            Need { pool: Pool::Digit, labels: DIGITS, how: How::Sum, required: true },
            Need { pool: Pool::DigitSmall, labels: DIGITS, how: How::Sum, required: false },
        ],
    },
    Screen {
        name: "ウデマエの精算",
        show: "バンカラマッチ（チャレンジ）を終えた後（灰色のゲージとポイント、TOTAL）",
        gives: "ウデマエポイントの変動（power）",
        needs: &[
            Need { pool: Pool::DigitGauge, labels: DIGITS, how: How::Sum, required: true },
            Need { pool: Pool::DigitTotal, labels: DIGITS, how: How::Sum, required: true },
            Need { pool: Pool::UdemaeTitle, labels: &["finish", "clear"], how: How::Manual, required: false },
        ],
    },
    Screen {
        name: "昇格",
        show: "昇格戦に勝ったとき（「昇格おめでとう!!」と「300p ウデマエポイントはリセットされます」）",
        gives: "昇格でポイントが 300p に戻ったこと",
        needs: &[Need { pool: Pool::UdemaeTitle, labels: &["promoted"], how: How::Manual, required: true }],
    },
    Screen {
        name: "進行（WIN LOSE）",
        show: "X マッチ・チャレンジの各試合の後（○の判子とイカが並ぶ画面）",
        gives: "今のセットの勝ち負けの数（set_progress）",
        needs: &[Need { pool: Pool::ProgressLabel, labels: &["win_lose"], how: How::Manual, required: true }],
    },
];

#[derive(Serialize)]
pub struct Item {
    pub pool: &'static str,
    pub label: String,
    /// 画面に出す名前（数字なら字そのもの）
    pub name: String,
    /// 手で登録した数・自動で足された数
    pub manual: usize,
    pub auto: usize,
    pub how: How,
    pub required: bool,
}

#[derive(Serialize)]
pub struct ScreenStatus {
    pub name: &'static str,
    pub show: &'static str,
    pub gives: &'static str,
    pub items: Vec<Item>,
    /// 無いと読めないものが全部そろっている
    pub ready: bool,
    /// 手で取る必要があって、まだ無いものがある
    pub needs_manual: bool,
}

/// ラベルの表示名（templates.rs の場所の定義から）
fn label_name(pool: Pool, label: &str) -> String {
    PLACES
        .iter()
        .filter(|p| p.pool == pool)
        .find_map(|p| match p.kind {
            Kind::Labels(l) => l.iter().find(|(id, _)| *id == label).map(|(_, n)| n.to_string()),
            Kind::Glyphs => None,
        })
        .unwrap_or_else(|| label.to_string())
}

pub fn status(t: &Templates) -> Vec<ScreenStatus> {
    SCREENS
        .iter()
        .map(|s| {
            let mut items = Vec::new();
            for n in s.needs {
                let labels: Vec<(String, String)> = if n.labels.is_empty() {
                    let mut d: Vec<(String, String)> = ('0'..='9').map(|c| (c.to_string(), c.to_string())).collect();
                    if n.pool == Pool::DigitSmall {
                        d.insert(0, (glyph_label('+').unwrap(), "+".into()));
                    }
                    d
                } else {
                    n.labels.iter().map(|l| (l.to_string(), label_name(n.pool, l))).collect()
                };
                for (label, name) in labels {
                    let list = t.get(n.pool).iter().filter(|tm| tm.label == label);
                    let (auto, manual): (Vec<_>, Vec<_>) = list.partition(|tm| is_auto(&tm.id));
                    items.push(Item {
                        pool: n.pool.dir_name(),
                        label,
                        name,
                        manual: manual.len(),
                        auto: auto.len(),
                        how: n.how,
                        required: n.required,
                    });
                }
            }
            let have = |i: &Item| i.manual + i.auto > 0;
            ScreenStatus {
                name: s.name,
                show: s.show,
                gives: s.gives,
                ready: items.iter().filter(|i| i.required && i.how != How::Shape).all(have),
                needs_manual: items.iter().any(|i| i.how == How::Manual && i.required && !have(i)),
                items,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_templates_list_everything_as_missing() {
        let st = status(&Templates::default());
        assert!(st.iter().all(|s| s.items.iter().all(|i| i.manual + i.auto == 0)));
        let menu = &st[0];
        assert_eq!(menu.items.iter().filter(|i| i.pool == "digit_menu").count(), 10);
        assert!(!menu.ready);
        // 手で取る必要があるのは、昇格と進行だけ（ほかは見本が無くても読めるか、自動でそろう）
        let manual: Vec<&str> = st.iter().filter(|s| s.needs_manual).map(|s| s.name).collect();
        assert_eq!(manual, ["昇格", "進行（WIN LOSE）"]);
        // 増減の数字には「+」も入る
        let x = st.iter().find(|s| s.name == "X パワーの変動").unwrap();
        assert!(x.items.iter().any(|i| i.pool == "digit_small" && i.name == "+"));
    }

    /// 手元の見本のそろい具合を出す（`-- --ignored print_my_status --nocapture`）
    #[test]
    #[ignore]
    fn print_my_status() {
        let t = Templates::load(&Templates::default_dir()).unwrap();
        for s in status(&t) {
            let mark = if s.needs_manual { "手で取る必要あり" } else if s.ready { "そろった" } else { "自動でそろう途中" };
            let items: Vec<String> = s
                .items
                .iter()
                .map(|i| format!("{}{}", i.name, if i.manual > 0 { "○" } else if i.auto > 0 { "◎" } else { "・" }))
                .collect();
            println!("{}【{mark}】 {}", s.name, items.join(" "));
        }
    }
}
