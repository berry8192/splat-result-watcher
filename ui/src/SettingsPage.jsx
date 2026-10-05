import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LAYOUTS, BGS } from "./DisplayApp.jsx";

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
  const setD = (k, v) => setForm({ ...form, display: { ...form.display, [k]: v } });
  const needsRestart = saved && (form.port !== saved.port || form.width !== saved.width);
  const obsChanged =
    saved && (form.capture_from !== saved.capture_from || form.obs_port !== saved.obs_port || form.obs_password !== saved.obs_password);

  const save = async () => {
    try {
      await invoke("save_settings", { settings: form });
      setSaved(form);
      setMsg({ bad: false, text: needsRestart || obsChanged ? "残した。番号・撮る幅・撮る配信ソフトは起動し直すと効く" : "残した" });
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

      <h3>見せる窓</h3>
      <div className="settings-form">
        <label>
          置き方
          <select value={form.display.layout} onChange={(e) => setD("layout", e.target.value)}>
            {Object.entries(LAYOUTS).map(([id, name]) => (
              <option key={id} value={id}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label>
          背景
          <select value={form.display.bg} onChange={(e) => setD("bg", e.target.value)}>
            {BGS.map(([c, name]) => (
              <option key={c} value={c}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label>
          字の大きさ（px）: モードとルール
          <input type="number" min="8" max="200" value={form.display.font_head} onChange={(e) => setD("font_head", Number(e.target.value))} />
          　パワー
          <input type="number" min="8" max="400" value={form.display.font_power} onChange={(e) => setD("font_power", Number(e.target.value))} />
          　勝敗
          <input type="number" min="8" max="200" value={form.display.font_set} onChange={(e) => setD("font_set", Number(e.target.value))} />
          <span className="small">　窓の大きさは字に合わせて変わる。「残す」ですぐ効く</span>
        </label>
        <label>
          縁取り（px、0 で無し）
          <input type="number" min="0" max="20" value={form.display.outline_px} onChange={(e) => setD("outline_px", Number(e.target.value))} />
          　色
          <input type="color" value={form.display.outline_color} onChange={(e) => setD("outline_color", e.target.value)} />
          <span className="small">　透明な背景のときに字を読みやすくする</span>
        </label>
      </div>

      <h3>設定</h3>
      <div className="settings-form">
        <label>
          待ち受けの番号（ws://127.0.0.1:番号/events）
          <input type="number" value={form.port} onChange={(e) => set("port", Number(e.target.value))} />
        </label>
        <label>
          撮る配信ソフト
          <select value={form.capture_from} onChange={(e) => set("capture_from", e.target.value)}>
            <option value="auto">自動（N Air が起きていればそれ、無ければ OBS）</option>
            <option value="n_air">N Air</option>
            <option value="obs">OBS Studio</option>
          </select>
        </label>
        <label>
          OBS の WebSocket の番号
          <input type="number" value={form.obs_port} onChange={(e) => set("obs_port", Number(e.target.value))} />
          <span className="small">　OBS の「ツール → WebSocket サーバー設定」で有効にする（既定 4455）</span>
        </label>
        <label>
          OBS の WebSocket のパスワード
          <input type="password" value={form.obs_password} onChange={(e) => set("obs_password", e.target.value)} />
          <span className="small">　OBS 側で「認証を有効にする」を切っていれば空でよい</span>
        </label>
        <label>
          配信の出力を撮る幅
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
        <label>
          <input type="checkbox" checked={form.hit_log} onChange={(e) => set("hit_log", e.target.checked)} /> 当たりの記録を残す
          （デバッグ用。何かに当たった画面と読みを hits\日付\ に。上限 500MB。起動し直すと効く）
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
