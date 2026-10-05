import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

// 見せる窓: 配信ソフトのウィンドウキャプチャで配信に載せる前提の、字だけの小さな窓。
// 枠は無く、左ドラッグで動かし、右クリックのメニューで置き方・背景・設定を変える。

const MODE_NAMES = { x: "Xマッチ", bankara_challenge: "バンカラ チャレンジ", bankara_open: "バンカラ オープン", bankara: "バンカラ", other: "" };
const RULE_NAMES = { area: "ガチエリア", yagura: "ガチヤグラ", hoko: "ガチホコ", asari: "ガチアサリ", turf_war: "ナワバリバトル" };
/** 置き方ごとの窓の大きさ（論理 px） */
const LAYOUTS = { yoko: { name: "横長", w: 560, h: 150 }, tate: { name: "縦長", w: 320, h: 300 } };
const BGS = [
  ["#16161d", "暗い"],
  ["#000000", "黒"],
  ["#00ff00", "緑（クロマキー用）"],
  ["#0000ff", "青（クロマキー用）"],
];

const store = {
  get(k, d) {
    try {
      return localStorage.getItem(k) ?? d;
    } catch {
      return d;
    }
  },
  set(k, v) {
    try {
      localStorage.setItem(k, v);
    } catch {
      /* 覚えられなくても動く */
    }
  },
};

/** 出来事（新しい順）から、見せるものを拾う。それぞれ一番新しいものだけ */
function pick(events) {
  let mode = null;
  let rule = null;
  let xp = null;
  let udemae = null;
  let set = null;
  for (const e of events) {
    if (mode == null && (e.type === "result" || e.type === "battle_started") && e.mode) mode = e.mode;
    if (mode == null && e.type === "observed") mode = e.kind === "x" ? "x" : "bankara";
    if (rule == null && e.rule && (e.type === "result" || e.type === "battle_started" || e.type === "observed")) rule = e.rule;
    if (e.type === "power" && !e.calibrating && e.after != null) {
      if (e.kind === "x" && xp == null) xp = e.after;
      if (e.kind === "udemae" && udemae == null) udemae = e.after;
    }
    if (e.type === "observed" && e.value != null) {
      if (e.kind === "x" && xp == null) xp = e.value;
      if (e.kind === "udemae" && udemae == null) udemae = e.value;
    }
    if (set == null && e.wins != null && e.losses != null && (e.type === "set_progress" || e.type === "observed")) {
      set = { wins: e.wins, losses: e.losses };
    }
    // パワーの変動が出たらセットは終わっている（3 勝目・3 敗目の後は進行の画面が出ない）。次のセットが見えるまで出さない
    if (set == null && e.type === "power" && !e.calibrating && e.before != null) set = { done: true };
  }
  if (set?.done) set = null;
  return { mode, rule, xp, udemae, set };
}

export default function DisplayApp() {
  const [s, setS] = useState(null);
  const [layout, setLayout] = useState(() => (LAYOUTS[store.get("srw_layout", "yoko")] ? store.get("srw_layout", "yoko") : "yoko"));
  const [bg, setBg] = useState(() => store.get("srw_bg", BGS[0][0]));
  const [menu, setMenu] = useState(null);

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

  // 置き方に合わせて窓の大きさを変える
  useEffect(() => {
    const { w, h } = LAYOUTS[layout];
    getCurrentWindow().setSize(new LogicalSize(w, h)).catch(() => {});
    store.set("srw_layout", layout);
  }, [layout]);
  useEffect(() => store.set("srw_bg", bg), [bg]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  const onMouseDown = (e) => {
    if (e.button === 0 && !menu) getCurrentWindow().startDragging().catch(() => {});
  };
  const onContextMenu = (e) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const { mode, rule, xp, udemae, set } = pick(s?.events ?? []);
  const head = [MODE_NAMES[mode] ?? "", RULE_NAMES[rule] ?? ""].filter(Boolean).join(" ");
  const isX = mode === "x";
  const isBankara = mode != null && mode.startsWith("bankara");
  const power = isX ? xp : isBankara ? udemae : null;
  const showSet = set && mode !== "bankara_open" && mode !== "other";
  const note = !s ? "" : !s.source ? "N Air か OBS が見つからない" : s.events.length === 0 ? "まだ試合を読んでいない" : "";

  return (
    <div className={`disp disp-${layout}`} style={{ background: bg }} onMouseDown={onMouseDown} onContextMenu={onContextMenu}>
      <div className="disp-top">
        <span className="disp-head">{head || " "}</span>
        {power != null && (
          <span className="disp-power">
            {isX ? power.toFixed(1) : power}
            {isBankara && <small>p</small>}
          </span>
        )}
      </div>
      {showSet && (
        <span className="disp-set">
          {set.wins}勝 {set.losses}敗
        </span>
      )}
      {note && <span className="disp-note">{note}</span>}

      {menu && (
        <div className="ctx" style={{ left: menu.x, top: menu.y }} onMouseDown={(e) => e.stopPropagation()}>
          {Object.entries(LAYOUTS).map(([id, l]) => (
            <button key={id} className={layout === id ? "on" : ""} onClick={() => (setLayout(id), setMenu(null))}>
              {l.name}
            </button>
          ))}
          <hr />
          {BGS.map(([c, name]) => (
            <button key={c} className={bg === c ? "on" : ""} onClick={() => (setBg(c), setMenu(null))}>
              背景: {name}
            </button>
          ))}
          <hr />
          <button onClick={() => (invoke("open_settings"), setMenu(null))}>設定・見本を開く</button>
          <button onClick={() => getCurrentWindow().minimize()}>しまう</button>
          <button onClick={() => getCurrentWindow().close()}>終了</button>
        </div>
      )}
    </div>
  );
}
