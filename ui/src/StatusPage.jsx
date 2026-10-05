import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const STAGE_NAMES = {
  no_signal: "映像なし",
  idle: "待機",
  in_battle: "バトル中",
  reading: "結果を認識中",
  post_match: "試合後",
};

/** 出来事を 1 行で */
function describe(ev) {
  switch (ev.type) {
    case "battle_started":
      return `試合開始 ${ev.rule ?? ""}`;
    case "result":
      return `結果 ${ev.outcome}（${ev.mode ?? "?"} ${ev.rule ?? ""}）`;
    case "power":
      if (ev.calibrating) return `${ev.kind} 計測中`;
      return `${ev.kind} ${ev.before ?? "?"} → ${ev.after}`;
    case "set_progress":
      return `セット進行 ${ev.wins}-${ev.losses}`;
    case "observed":
      return `観測値 ${ev.kind} ${ev.value ?? ""}${ev.wins != null ? `（${ev.wins}-${ev.losses}）` : ""}`;
    default:
      return ev.type;
  }
}

export default function StatusPage() {
  const [s, setS] = useState(null);
  const [frame, setFrame] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const [st, fr] = await Promise.all([invoke("status"), invoke("frame", { maxW: 640 })]);
        if (!alive) return;
        setS(st);
        setFrame(fr);
        setError(null);
      } catch (e) {
        if (alive) setError(String(e));
      }
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  if (!s) return <div className="page">{error ?? "読み込み中…"}</div>;

  return (
    <div className="page status">
      <div className="row">
        <div className="col preview">
          {frame ? <img src={frame} alt="ゲーム画面" /> : <div className="noframe">まだキャプチャできていません</div>}
          <div className="seen">
            <div>
              認識結果: <b>{s.seen}</b>
            </div>
            {s.notes.map((n, i) => (
              <div key={i} className="note">
                {n}
              </div>
            ))}
          </div>
        </div>
        <div className="col facts">
          <div className={`stage stage-${s.stage}`}>{STAGE_NAMES[s.stage] ?? s.stage}</div>
          <table>
            <tbody>
              <tr>
                <th>待ち受け</th>
                <td>
                  {s.addr}（接続 {s.clients}）
                </td>
              </tr>
              {s.server_error && (
                <tr>
                  <th>サーバ</th>
                  <td className="bad">{s.server_error}</td>
                </tr>
              )}
              <tr>
                <th>キャプチャ</th>
                <td>
                  {s.source ? `${s.source} からキャプチャ中` : "キャプチャできていません（N Air または OBS を起動してください）"}・平均 {s.capture_ms.toFixed(1)}ms・
                  {s.interval_ms}ms 間隔
                </td>
              </tr>
              <tr>
                <th>最新の seq</th>
                <td>{s.last_seq}</td>
              </tr>
              <tr>
                <th>録画</th>
                <td>
                  <label>
                    <input type="checkbox" checked={s.record} onChange={(e) => invoke("set_record", { on: e.target.checked })} />{" "}
                    有効
                  </label>
                  {s.record_dir && <div className="small">{s.record_dir}</div>}
                </td>
              </tr>
            </tbody>
          </table>
          <h3>送信したイベント</h3>
          <ul className="events">
            {s.events.length === 0 && <li className="small">まだありません</li>}
            {s.events.map((ev) => (
              <li key={ev.seq}>
                <span className="seq">#{ev.seq}</span> {describe(ev)} <span className="small">{ev.match_id}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>
      <details className="sec">
        <summary>ログ</summary>
        <pre className="log">{s.log.join("\n")}</pre>
      </details>
    </div>
  );
}
