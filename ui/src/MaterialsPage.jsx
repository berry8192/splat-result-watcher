import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** どうそろうか */
const HOW = {
  shape: "見本なしでも形と色で読める。確かめられたら自動で足す",
  sum: "手がかりの数字で推測し、計算が合ったら自動で足す",
  same_value: "試合後の値とメニューの値が同じことで自動で足す",
  manual: "手で登録する（「見本の登録」で）",
};
const HOW_SHORT = { shape: "形", sum: "計算", same_value: "同じ値", manual: "手で" };

const have = (i) => i.manual + i.auto > 0;

function Chip({ i }) {
  const cls = i.manual > 0 ? "chip manual" : i.auto > 0 ? "chip auto" : i.how === "manual" ? "chip missing-manual" : "chip missing";
  const title =
    `${i.name}（${i.pool}）: 手で ${i.manual}・自動 ${i.auto}。` + HOW[i.how] + (i.required ? "" : "（無くても読める・あると確か）");
  return (
    <span className={cls} title={title}>
      {i.name}
      {i.auto > 0 && i.manual === 0 && <em>自動</em>}
    </span>
  );
}

export default function MaterialsPage() {
  const [list, setList] = useState(null);
  const [error, setError] = useState(null);

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

  const manualLeft = list.flatMap((s) => s.items.filter((i) => i.how === "manual" && i.required && !have(i)).map((i) => ({ s, i })));
  const autoLeft = list.flatMap((s) => s.items.filter((i) => i.how !== "manual" && i.how !== "shape" && i.required && !have(i)));

  return (
    <div className="page materials">
      <div className="summary">
        {manualLeft.length === 0 ? (
          <div className="good">手で取る必要があるものは、もう無い</div>
        ) : (
          <div>
            <b>手で取る必要があるもの</b>:{" "}
            {manualLeft.map(({ s, i }) => `${i.name}（${s.name}）`).join("、")}
            <div className="small">その画面を映して、「見本の登録」の「直近の画面から選ぶ」で戻って登録する</div>
          </div>
        )}
        <div>
          {autoLeft.length === 0 ? (
            <span className="good">自動でそろうものも、全部そろった</span>
          ) : (
            <span>
              自動でそろうのを待っているもの: <b>{autoLeft.length}</b> 字（遊んでいるうちに、計算や同じ値で足される）
            </span>
          )}
        </div>
        <div className="legend small">
          <span className="chip manual">手で登録済み</span>
          <span className="chip auto">
            自動で足された<em>自動</em>
          </span>
          <span className="chip missing">まだ（自動でそろう）</span>
          <span className="chip missing-manual">まだ（手で取る）</span>
          　字にマウスを乗せると詳しく出る
        </div>
      </div>

      {list.map((s) => (
        <div key={s.name} className={`screen-card ${s.needs_manual ? "needs-manual" : s.ready ? "ready" : "waiting"}`}>
          <div className="screen-head">
            <b>{s.name}</b>
            <span className="badge">{s.needs_manual ? "手で取る必要あり" : s.ready ? "そろった" : "自動でそろう途中"}</span>
          </div>
          <div className="small">映し方: {s.show}</div>
          <div className="small">読めるもの: {s.gives}</div>
          {groups(s.items).map((g) => (
            <div key={g.key} className="chips">
              <span className="how small">
                {HOW_SHORT[g.how]}
                {g.required ? "" : "・任意"}
              </span>
              {g.items.map((i) => (
                <Chip key={i.pool + i.label} i={i} />
              ))}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

/** 同じ見本の山・同じそろい方をまとめて 1 行に */
function groups(items) {
  const out = [];
  for (const i of items) {
    const key = `${i.pool}/${i.how}`;
    const g = out.find((g) => g.key === key);
    if (g) g.items.push(i);
    else out.push({ key, how: i.how, required: i.required, items: [i] });
  }
  return out;
}
