import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** どうそろうか */
const HOW = {
  shape: "見本が無くても形と色で読める。別の画面で確かめられたら自動で足す",
  sum: "手がかりの数字で推測し、「動く前 + 増減 = 動いた後」が合ったら自動で足す",
  same_value: "試合後の値とメニューの値が同じことで自動で足す",
  manual: "手で登録する（「見本の登録」で、その画面を映して登録）",
};

const have = (i) => i.manual + i.auto > 0;
const missing = (s, pred) => s.items.filter((i) => !have(i) && pred(i));

/** 画面ごとの状態: DONE（無いと読めないものが全部ある）/ WAIT（自動で足されるのを待つ）/ MANUAL（手で取る必要あり） */
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
          {manualCount === 0 ? "MANUAL は無し" : `MANUAL は ${manualCount} 件`}。{waitCount === 0 ? "WAIT も無し" : `WAIT は ${waitCount} 字（遊んでいるうちに足される）`}
        </span>
        <span className="small">行を押すと、映し方と足され方が出る</span>
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
                <div>映し方: {s.show}</div>
                <div>読めるもの: {s.gives}</div>
                {hows.map((h) => (
                  <div key={h}>足され方: {HOW[h]}</div>
                ))}
                {optional.length > 0 && <div>無くても読めるもの（あると確か）: {optional.map((i) => i.name).join(" ")}</div>}
                {wait.length > 0 && st !== "WAIT" && <div>待ち: {wait.map((i) => i.name).join(" ")}</div>}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
