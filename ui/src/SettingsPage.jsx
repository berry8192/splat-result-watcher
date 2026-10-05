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
      setMsg({ bad: false, text: needsRestart || obsChanged ? "保存しました。ポート・キャプチャの幅・キャプチャ元は再起動後に反映されます" : "保存しました" });
    } catch (e) {
      setMsg({ bad: true, text: String(e) });
    }
  };

  const resetGame = async () => {
    if (!window.confirm("現在の試合をイベントを送信せずに破棄し、待機に戻します。よろしいですか？")) return;
    await invoke("reset_game");
    setMsg({ bad: false, text: "現在の試合を破棄しました" });
  };

  return (
    <div className="page">
      <h3>テンプレートの不足</h3>
      {status && status.template_gaps.length === 0 && <div className="good">不足しているテンプレートはありません</div>}
      {status && status.template_gaps.length > 0 && (
        <>
          <ul className="gaps">
            {status.template_gaps.map((g) => (
              <li key={g}>{g}</li>
            ))}
          </ul>
          <div className="small">「テンプレートの登録」で追加してください。数字は 1 枚の画面に表示されている文字のみ登録できるため、複数の画面から登録します</div>
        </>
      )}

      <h3>表示ウィンドウ</h3>
      <div className="settings-form">
        <label>
          レイアウト
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
          文字サイズ（px）: モードとルール
          <input type="number" min="8" max="200" value={form.display.font_head} onChange={(e) => setD("font_head", Number(e.target.value))} />
          　パワー
          <input type="number" min="8" max="400" value={form.display.font_power} onChange={(e) => setD("font_power", Number(e.target.value))} />
          　勝敗
          <input type="number" min="8" max="200" value={form.display.font_set} onChange={(e) => setD("font_set", Number(e.target.value))} />
          <span className="small">　ウィンドウの大きさは文字に合わせて自動で変わります。「保存」で即時に反映されます</span>
        </label>
        <label>
          文字色
          <input type="color" value={form.display.text_color} onChange={(e) => setD("text_color", e.target.value)} />
          　モードとルールの色
          <input type="color" value={form.display.head_color} onChange={(e) => setD("head_color", e.target.value)} />
        </label>
        <label>
          縁取り（px、0 で無し）
          <input type="number" min="0" max="20" value={form.display.outline_px} onChange={(e) => setD("outline_px", Number(e.target.value))} />
          　色
          <input type="color" value={form.display.outline_color} onChange={(e) => setD("outline_color", e.target.value)} />
          <span className="small">　背景が透明のときに文字を読みやすくします</span>
        </label>
      </div>

      <h3>設定</h3>
      <div className="settings-form">
        <label>
          待ち受けポート（ws://127.0.0.1:ポート/events）
          <input type="number" value={form.port} onChange={(e) => set("port", Number(e.target.value))} />
        </label>
        <label>
          キャプチャ元
          <select value={form.capture_from} onChange={(e) => set("capture_from", e.target.value)}>
            <option value="auto">自動（N Air があればそれを、なければ OBS を使用）</option>
            <option value="n_air">N Air</option>
            <option value="obs">OBS Studio</option>
          </select>
        </label>
        <label>
          OBS の WebSocket ポート
          <input type="number" value={form.obs_port} onChange={(e) => set("obs_port", Number(e.target.value))} />
          <span className="small">　OBS の「ツール → WebSocket サーバー設定」で有効にしてください（既定 4455）</span>
        </label>
        <label>
          OBS の WebSocket のパスワード
          <input type="password" value={form.obs_password} onChange={(e) => set("obs_password", e.target.value)} />
          <span className="small">　OBS 側で認証を無効にしている場合は空のままで構いません</span>
        </label>
        <label>
          キャプチャの幅
          <input type="number" value={form.width} onChange={(e) => set("width", Number(e.target.value))} />
          <span className="small">　1280 を推奨します（ゲーム画面が照合サイズと一致します。1920 では負荷が高くなります）</span>
        </label>
        <label>
          <input type="checkbox" checked={form.record} onChange={(e) => set("record", e.target.checked)} /> フレームを録画する
          （samples/record に 0.5 秒ごとのゲーム画面を保存します）
        </label>
        <label>
          録画の上限（GB）
          <input
            type="number"
            step="1"
            value={form.record_cap_gb}
            onChange={(e) => set("record_cap_gb", Number(e.target.value))}
          />
          <span className="small">　超過分は古い順に削除します。次回の録画開始時に反映されます</span>
        </label>
        <label>
          <input type="checkbox" checked={form.hit_log} onChange={(e) => set("hit_log", e.target.checked)} /> 検出ログを保存する
          （デバッグ用。検出した画面と認識結果を hits\日付\ に保存します。上限 500MB。再起動後に反映されます）
        </label>
        <button onClick={save}>保存</button>
        {needsRestart && <span className="small">　ポートとキャプチャの幅は再起動後に反映されます</span>}
        {msg && <span className={msg.bad ? "bad" : "good"}>　{msg.text}</span>}
      </div>

      <h3>トラブル時</h3>
      <div>
        <button className="danger" onClick={resetGame}>
          現在の試合を破棄して待機に戻す
        </button>
        <span className="small">　状態が進まなくなったときに使用します。イベントは送信せずに破棄します（誤認識した勝敗は nicomment 側で修正してください）</span>
      </div>
    </div>
  );
}
