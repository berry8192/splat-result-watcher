import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const STAGE_NAMES = {
  no_signal: "映像なし",
  idle: "待機",
  in_battle: "バトル中",
  reading: "結果を読み中",
  post_match: "試合後",
};
const MODE_NAMES = {
  x: "Xマッチ",
  bankara_challenge: "バンカラ(チャレンジ)",
  bankara_open: "バンカラ(オープン)",
  other: "その他",
};
const RULE_NAMES = { area: "ガチエリア", yagura: "ガチヤグラ", hoko: "ガチホコ", asari: "ガチアサリ" };
const OUTCOME_NAMES = {
  win: "WIN",
  lose: "LOSE",
  no_contest: "無効試合",
  lose_uncounted: "LOSE（数えない）",
};

/** 出来事（新しい順）から、見せるものを拾う */
function pick(events) {
  const first = (f) => events.find(f);
  const result = first((e) => e.type === "result");
  const xp = first((e) => e.type === "power" && e.kind === "x");
  const udemae = first((e) => e.type === "power" && e.kind === "udemae");
  const progress = first((e) => e.type === "set_progress");
  return { result, xp, udemae, progress };
}

function time(at) {
  if (!at) return "";
  const d = new Date(at);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function delta(before, after) {
  if (before == null || after == null) return null;
  const d = Math.round((after - before) * 10) / 10;
  return d >= 0 ? `+${d}` : `${d}`;
}

export default function DisplayApp() {
  const [s, setS] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    let alive = true;
    const tick = () =>
      invoke("status")
        .then((st) => alive && (setS(st), setError(null)))
        .catch((e) => alive && setError(String(e)));
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  if (!s) return <div className="display">{error ?? "読み込み中…"}</div>;
  const { result, xp, udemae, progress } = pick(s.events);
  const xd = xp && !xp.calibrating ? delta(xp.before, xp.after) : null;
  const ud = udemae ? delta(udemae.before, udemae.after) : null;

  return (
    <div className="display">
      <div className="display-head">
        <span className={`stage small-stage stage-${s.stage}`}>{STAGE_NAMES[s.stage] ?? s.stage}</span>
        <span className="small">
          {s.clients > 0 ? `受け手 ${s.clients}` : "受け手なし"}
          {s.server_error ? "・サーバ停止" : ""}
        </span>
        <button className="gear" onClick={() => invoke("open_settings")} title="設定・見本の登録">
          設定・見本
        </button>
      </div>

      {result ? (
        <div className="last-result">
          <div className={`outcome outcome-${result.outcome}`}>{OUTCOME_NAMES[result.outcome] ?? result.outcome}</div>
          <div className="small">
            {MODE_NAMES[result.mode] ?? result.mode ?? ""} {RULE_NAMES[result.rule] ?? ""} {time(result.ended_at)}
          </div>
        </div>
      ) : (
        <div className="last-result small">まだ試合を読んでいない</div>
      )}

      <table className="display-facts">
        <tbody>
          {progress && (
            <tr>
              <th>進行</th>
              <td>
                {progress.wins} 勝 {progress.losses} 敗
              </td>
            </tr>
          )}
          {xp && (
            <tr>
              <th>Xパワー</th>
              <td>
                {xp.calibrating ? (
                  "計測中"
                ) : (
                  <>
                    <b>{xp.after?.toFixed(1)}</b> {xd && <span className={xd.startsWith("+") ? "good" : "bad"}>{xd}</span>}
                  </>
                )}
              </td>
            </tr>
          )}
          {udemae && (
            <tr>
              <th>ウデマエ</th>
              <td>
                <b>{udemae.after}p</b> {ud && <span className={ud.startsWith("+") ? "good" : "bad"}>{ud}</span>}
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
