import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** どうそろうか */
const HOW = {
  shape: "テンプレートがなくても形状と色で判定できます。他の画面で裏付けが取れた時点で自動登録されます",
  sum: "参照フォントで推定し、「変動前 + 増減 = 変動後」の計算が一致した時点で自動登録されます",
  same_value: "試合後の値とメニューの値が一致することを根拠に自動登録されます",
  manual: "手動で登録します（「テンプレートの登録」で、その画面を表示して登録）",
};

const have = (i) => i.manual + i.auto > 0;
const missing = (s, pred) => s.items.filter((i) => !have(i) && pred(i));

/** 画面ごとの状態: DONE（必須のテンプレートがそろっている）/ WAIT（自動登録待ち）/ MANUAL（手動登録が必要） */
function status(s) {
  if (s.needs_manual) return "MANUAL";
  if (s.ready) return "DONE";
  return "WAIT";
}

export default function MaterialsPage() {
  const [list, setList] = useState(null);
  const [error, setError] = useState(null);
  const [open, setOpen] = useState(() => new Set());

  useEffect(() => {
    let alive = true;
    const tick = () =>
      invoke("materials")
        .then((l) => alive && setList(l))
        .catch((e) => alive && setError(String(e)));
    tick();
    // 遊んでいる間に自動で足されていくので、ときどき読み直す
    const id = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  if (!list) return <div className="page">{error ?? "読み込み中…"}</div>;

  const manualCount = list.reduce((n, s) => n + missing(s, (i) => i.how === "manual" && i.required).length, 0);
  const waitCount = list.reduce((n, s) => n + missing(s, (i) => i.how !== "manual" && i.required).length, 0);
  const toggle = (name) =>
    setOpen((o) => {
      const n = new Set(o);
      n.has(name) ? n.delete(name) : n.add(name);
      return n;
    });

  return (
    <div className="page materials">
      <div className="mat-sum">
        <span>
          MANUAL: {manualCount === 0 ? "なし" : `${manualCount} 件`}　WAIT: {waitCount === 0 ? "なし" : `${waitCount} 文字（プレイ中に自動登録されます）`}
        </span>
        <span className="small">行をクリックすると詳細を表示します</span>
      </div>
      {list.map((s) => {
        const st = status(s);
        const wait = missing(s, (i) => i.how !== "manual" && i.required);
        const manual = missing(s, (i) => i.how === "manual" && i.required);
        const optional = missing(s, (i) => !i.required);
        const hows = [...new Set(s.items.map((i) => i.how))];
        return (
          <div key={s.name} className={`mat-row ${open.has(s.name) ? "open" : ""}`}>
            <button className="mat-line" onClick={() => toggle(s.name)}>
              <span className={`mat-dot ${st.toLowerCase()}`} />
              <span className="mat-name">{s.name}</span>
              <span className={`mat-st ${st.toLowerCase()}`}>
                {st}
                {st === "WAIT" && <b>{wait.map((i) => i.name).join(" ")}</b>}
                {st === "MANUAL" && <b>{manual.map((i) => i.name).join(" ")}</b>}
              </span>
            </button>
            {open.has(s.name) && (
              <div className="mat-detail">
                <div>表示方法: {s.show}</div>
                <div>取得できる情報: {s.gives}</div>
                {hows.map((h) => (
                  <div key={h}>登録方法: {HOW[h]}</div>
                ))}
                {optional.length > 0 && <div>任意（あると精度が上がります）: {optional.map((i) => i.name).join(" ")}</div>}
                {wait.length > 0 && st !== "WAIT" && <div>自動登録待ち: {wait.map((i) => i.name).join(" ")}</div>}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
