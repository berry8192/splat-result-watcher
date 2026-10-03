import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function SettingsPage() {
  const [form, setForm] = useState(null);
  const [saved, setSaved] = useState(null);
  const [status, setStatus] = useState(null);
  const [msg, setMsg] = useState(null);

  useEffect(() => {
    invoke("get_settings").then((s) => {
      setForm(s);
      setSaved(s);
    });
    let alive = true;
    const tick = () => invoke("status").then((st) => alive && setStatus(st));
    tick();
    const id = setInterval(tick, 2000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  if (!form) return <div className="page">読み込み中…</div>;

  const set = (k, v) => setForm({ ...form, [k]: v });
  const needsRestart = saved && (form.port !== saved.port || form.width !== saved.width);

  const save = async () => {
    try {
      await invoke("save_settings", { settings: form });
      setSaved(form);
      setMsg({ bad: false, text: needsRestart ? "残した。番号と撮る幅は起動し直すと効く" : "残した" });
    } catch (e) {
      setMsg({ bad: true, text: String(e) });
    }
  };

  const resetGame = async () => {
    if (!window.confirm("今の試合を何も出さずに捨てて、待機に戻しますか？")) return;
    await invoke("reset_game");
    setMsg({ bad: false, text: "今の試合を捨てた" });
  };

  return (
    <div className="page">
      <h3>見本のそろい具合</h3>
      {status && status.template_gaps.length === 0 && <div className="good">足りない見本は無い</div>}
      {status && status.template_gaps.length > 0 && (
        <>
          <ul className="gaps">
            {status.template_gaps.map((g) => (
              <li key={g}>{g}</li>
            ))}
          </ul>
          <div className="small">「見本の登録」で足す。数字は 1 枚の画面に出ている字しか足せないので、何枚かの画面から足す</div>
        </>
      )}

      <h3>設定</h3>
      <div className="settings-form">
        <label>
          待ち受けの番号（ws://127.0.0.1:番号/events）
          <input type="number" value={form.port} onChange={(e) => set("port", Number(e.target.value))} />
        </label>
        <label>
          N Air の出力を撮る幅
          <input type="number" value={form.width} onChange={(e) => set("width", Number(e.target.value))} />
          <span className="small">　1280 を勧める（ゲーム穴が照合の大きさにちょうどなる。1920 だと撮影が重い）</span>
        </label>
        <label>
          <input type="checkbox" checked={form.record} onChange={(e) => set("record", e.target.checked)} /> 見本の録画を回す
          （samples/record に 0.5 秒ごとのゲーム穴を残す）
        </label>
        <label>
          録画の上限（GB）
          <input
            type="number"
            step="1"
            value={form.record_cap_gb}
            onChange={(e) => set("record_cap_gb", Number(e.target.value))}
          />
          <span className="small">　超えたら古い順に消す。次に録画を始めたときに効く</span>
        </label>
        <button onClick={save}>残す</button>
        {needsRestart && <span className="small">　番号と撮る幅は起動し直すと効く</span>}
        {msg && <span className={msg.bad ? "bad" : "good"}>　{msg.text}</span>}
      </div>

      <h3>困ったとき</h3>
      <div>
        <button className="danger" onClick={resetGame}>
          今の試合を捨てて待機に戻す
        </button>
        <span className="small">　段階がおかしなまま止まったとき。何も出さずに捨てる（読み間違いの勝敗は nicomment で直す）</span>
      </div>
    </div>
  );
}
