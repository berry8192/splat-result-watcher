import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { pick } from "./DisplayApp.jsx";

// 手動操作: 表示ウィンドウと受信側の値を手で直す。直した値は `manual` イベントとして送られる
// （差分ではなく直した後の値。何度届いても同じ結果になる）。

const MODE_NAMES = { x: "Xマッチ", bankara_challenge: "バンカラ チャレンジ", bankara_open: "バンカラ オープン", bankara: "バンカラ", other: "その他" };
const RULE_NAMES = { area: "ガチエリア", yagura: "ガチヤグラ", hoko: "ガチホコ", asari: "ガチアサリ", turf_war: "ナワバリバトル" };
const STEPS = [25, 50, 75];

export default function ManualPage() {
  const [s, setS] = useState(null);
  const [kindSel, setKindSel] = useState(null);
  const [text, setText] = useState("");
  const [msg, setMsg] = useState(null);

  useEffect(() => {
    let alive = true;
    const tick = () => invoke("status").then((st) => alive && setS(st)).catch(() => {});
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  if (!s) return <div className="page">読み込み中…</div>;

  const cur = pick(s.events);
  const kind = kindSel ?? (cur.mode && cur.mode.startsWith("bankara") ? "udemae" : "x");
  const isX = kind === "x";
  const value = isX ? cur.xp : cur.udemae;
  const wins = cur.set?.wins ?? 0;
  const losses = cur.set?.losses ?? 0;
  const fmt = (v) => (v == null ? "—" : isX ? v.toFixed(1) : `${v}p`);

  const send = async (fields) => {
    const ev = { kind, ...fields };
    if (isX && cur.rule) ev.rule = cur.rule;
    try {
      const seq = await invoke("manual", { ev });
      setMsg({ bad: false, text: `送信しました（#${seq}）` });
    } catch (e) {
      setMsg({ bad: true, text: String(e) });
    }
  };
  const setValue = (v) => send({ value: isX ? Math.round(v * 10) / 10 : Math.round(v) });
  const setSet = (w, l) => send({ wins: Math.max(0, w), losses: Math.max(0, l) });

  const resetGame = async () => {
    if (!window.confirm("現在の試合をイベントを送信せずに破棄し、待機に戻します。よろしいですか？")) return;
    await invoke("reset_game");
    setMsg({ bad: false, text: "現在の試合を破棄しました" });
  };

  return (
    <div className="page manual">
      <div className="manual-now">
        <span className="small">現在の表示</span>
        <span>
          {[MODE_NAMES[cur.mode], RULE_NAMES[cur.rule]].filter(Boolean).join(" ") || "モード不明"}
        </span>
        <span>
          <b>{fmt(value)}</b>　{wins}勝 {losses}敗
        </span>
      </div>

      <h3>パワー・ウデマエポイント</h3>
      <div className="manual-row">
        <select value={kind} onChange={(e) => setKindSel(e.target.value)}>
          <option value="x">X パワー</option>
          <option value="udemae">ウデマエポイント</option>
        </select>
        <input
          type="number"
          step={isX ? 0.1 : 1}
          value={text}
          placeholder={value == null ? "" : String(value)}
          onChange={(e) => setText(e.target.value)}
        />
        <button disabled={text === "" || Number.isNaN(Number(text))} onClick={() => (setValue(Number(text)), setText(""))}>
          この値にする
        </button>
      </div>
      <div className="manual-row">
        {STEPS.map((d) => (
          <button key={`-${d}`} disabled={value == null} onClick={() => setValue(value - d)}>
            −{d}
          </button>
        ))}
        {STEPS.map((d) => (
          <button key={`+${d}`} disabled={value == null} onClick={() => setValue(value + d)}>
            +{d}
          </button>
        ))}
      </div>

      <h3>セットの勝敗</h3>
      <div className="manual-row">
        <span>勝ち</span>
        <button onClick={() => setSet(wins - 1, losses)} disabled={wins === 0}>
          −
        </button>
        <b>{wins}</b>
        <button onClick={() => setSet(wins + 1, losses)}>＋</button>
        <span style={{ marginLeft: 16 }}>負け</span>
        <button onClick={() => setSet(wins, losses - 1)} disabled={losses === 0}>
          −
        </button>
        <b>{losses}</b>
        <button onClick={() => setSet(wins, losses + 1)}>＋</button>
        <button style={{ marginLeft: 16 }} onClick={() => setSet(0, 0)}>
          0勝 0敗に戻す
        </button>
      </div>

      <h3>試合の認識</h3>
      <div className="manual-row">
        <button className="danger" onClick={resetGame}>
          現在の試合を破棄して待機に戻す
        </button>
        <span className="small">状態が進まなくなったときに使用します。イベントは送信せずに破棄します</span>
      </div>

      {msg && <div className={msg.bad ? "bad" : "good"}>{msg.text}</div>}
      <div className="small" style={{ marginTop: 12 }}>
        直した値は表示ウィンドウに反映され、受信側にも `manual` イベントとして送られます（直した後の値を送るので、何度届いても同じ結果になります）。
      </div>
    </div>
  );
}
