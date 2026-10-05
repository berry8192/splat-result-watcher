import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LAYOUTS, BGS } from "./DisplayApp.jsx";

/** 折りたたみの開閉を覚える（覚えられなくても動く） */
function Section({ id, title, children }) {
  const key = `srw_sec_${id}`;
  const [open, setOpen] = useState(() => {
    try {
      return localStorage.getItem(key) === "1";
    } catch {
      return false;
    }
  });
  const toggle = (e) => {
    setOpen(e.target.open);
    try {
      localStorage.setItem(key, e.target.open ? "1" : "0");
    } catch {
      /* 覚えられなくてもよい */
    }
  };
  return (
    <details className="sec" open={open} onToggle={toggle}>
      <summary>{title}</summary>
      <div className="sec-body">{children}</div>
    </details>
  );
}

export default function SettingsPage() {
  const [form, setForm] = useState(null);
  const [saved, setSaved] = useState(null);
  const [msg, setMsg] = useState(null);

  useEffect(() => {
    invoke("get_settings").then((s) => {
      setForm(s);
      setSaved(s);
    });
  }, []);

  if (!form) return <div className="page">読み込み中…</div>;

  const set = (k, v) => setForm({ ...form, [k]: v });
  const setD = (k, v) => setForm({ ...form, display: { ...form.display, [k]: v } });
  const dirty = JSON.stringify(form) !== JSON.stringify(saved);
  const needsRestart =
    saved &&
    (form.port !== saved.port ||
      form.width !== saved.width ||
      form.capture_from !== saved.capture_from ||
      form.obs_port !== saved.obs_port ||
      form.obs_password !== saved.obs_password ||
      form.hit_log !== saved.hit_log);

  const save = async () => {
    try {
      await invoke("save_settings", { settings: form });
      setSaved(form);
      setMsg({ bad: false, text: needsRestart ? "保存しました。ポート・キャプチャ・検出ログの変更は再起動後に反映されます" : "保存しました" });
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
    <div className="page settings-form">
      <Section id="display" title="表示ウィンドウ">
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
        </label>
        <label>
          文字色: 「Xマッチ」
          <input type="color" value={form.display.x_color} onChange={(e) => setD("x_color", e.target.value)} />
          　「バンカラ」
          <input type="color" value={form.display.bankara_color} onChange={(e) => setD("bankara_color", e.target.value)} />
          　ルール
          <input type="color" value={form.display.head_color} onChange={(e) => setD("head_color", e.target.value)} />
          　パワー
          <input type="color" value={form.display.power_color} onChange={(e) => setD("power_color", e.target.value)} />
          　勝敗
          <input type="color" value={form.display.set_color} onChange={(e) => setD("set_color", e.target.value)} />
        </label>
        <label>
          縁取り（px、0 で無し）
          <input type="number" min="0" max="20" value={form.display.outline_px} onChange={(e) => setD("outline_px", Number(e.target.value))} />
          　色
          <input type="color" value={form.display.outline_color} onChange={(e) => setD("outline_color", e.target.value)} />
        </label>
        <div className="small">ウィンドウの大きさは文字に合わせて自動で変わります。配信には配信ソフトのウィンドウキャプチャで載せてください</div>
      </Section>

      <Section id="capture" title="キャプチャ">
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
          　パスワード
          <input type="password" value={form.obs_password} onChange={(e) => set("obs_password", e.target.value)} />
          <div className="small">OBS の「ツール → WebSocket サーバー設定」で有効にしてください（既定 4455）。認証を無効にしている場合、パスワードは空のままで構いません</div>
        </label>
        <label>
          キャプチャの幅
          <input type="number" value={form.width} onChange={(e) => set("width", Number(e.target.value))} />
          <span className="small">　1280 を推奨します（1920 では負荷が高くなります）</span>
        </label>
      </Section>

      <Section id="server" title="接続">
        <label>
          待ち受けポート（ws://127.0.0.1:ポート/events）
          <input type="number" value={form.port} onChange={(e) => set("port", Number(e.target.value))} />
        </label>
      </Section>

      <Section id="record" title="録画と検出ログ">
        <label>
          <input type="checkbox" checked={form.record} onChange={(e) => set("record", e.target.checked)} /> フレームを録画する
          <span className="small">　samples/record に 0.5 秒ごとのゲーム画面を保存します</span>
        </label>
        <label>
          録画の上限（GB）
          <input type="number" step="1" value={form.record_cap_gb} onChange={(e) => set("record_cap_gb", Number(e.target.value))} />
          <span className="small">　超過分は古い順に削除します</span>
        </label>
        <label>
          <input type="checkbox" checked={form.hit_log} onChange={(e) => set("hit_log", e.target.checked)} /> 検出ログを保存する
          <span className="small">　デバッグ用。検出した画面と認識結果を hits\日付\ に保存します（上限 500MB）</span>
        </label>
      </Section>

      <Section id="trouble" title="トラブル時">
        <button className="danger" onClick={resetGame}>
          現在の試合を破棄して待機に戻す
        </button>
        <span className="small">　状態が進まなくなったときに使用します。イベントは送信せずに破棄します</span>
      </Section>

      <div className="save-bar">
        <button onClick={save} disabled={!dirty}>
          保存
        </button>
        {dirty && <span className="small">　未保存の変更があります</span>}
        {msg && <span className={msg.bad ? "bad" : "good"}>　{msg.text}</span>}
      </div>
    </div>
  );
}
