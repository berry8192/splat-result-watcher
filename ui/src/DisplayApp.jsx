import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Menu, MenuItem, CheckMenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";

// 見せる窓: 配信ソフトのウィンドウキャプチャで配信に載せる前提の、字だけの小さな窓。
// 枠は無く、左ドラッグで動かし、右クリックのメニューで置き方・背景を変える（字の大きさは設定の窓で）。
// メニューは OS のもの（窓が小さいので、窓の中に描くと収まらない）。
// 置き方・背景・字の大きさは settings.json の `display` に入り、設定の窓と共有する。窓の大きさは中身に合わせる。

const MODE_NAMES = { x: "Xマッチ", bankara_challenge: "バンカラ チャレンジ", bankara_open: "バンカラ オープン", bankara: "バンカラ", other: "" };
const RULE_NAMES = { area: "ガチエリア", yagura: "ガチヤグラ", hoko: "ガチホコ", asari: "ガチアサリ", turf_war: "ナワバリバトル" };
export const LAYOUTS = { yoko: "横長", tate: "縦長" };
export const BGS = [
  ["transparent", "透明（ウィンドウキャプチャで透過を許可する）"],
  ["#16161d", "暗い"],
  ["#000000", "黒"],
  ["#00ff00", "緑（クロマキー用）"],
  ["#0000ff", "青（クロマキー用）"],
];
/** 窓の余白（論理 px） */
const PAD = { x: 28, y: 22 };

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
    // パワーの変動が出たらセットは終わっている（3 勝目・3 敗目の後は進行の画面が出ない）。次のセットは 0 勝 0 敗から
    if (set == null && e.type === "power" && !e.calibrating && e.before != null) set = { wins: 0, losses: 0 };
  }
  return { mode, rule, xp, udemae, set };
}

/** 値が変わったら、前の値から新しい値へ 1.6 秒かけて数え上げる（ドラムロール）。終わったら `pulse` が進む */
function useRolling(value) {
  const [shown, setShown] = useState(value);
  const [pulse, setPulse] = useState(0);
  const prev = useRef(value);
  useEffect(() => {
    const from = prev.current;
    prev.current = value;
    if (value == null || from == null || from === value) {
      setShown(value);
      return;
    }
    const t0 = performance.now();
    const dur = 1600;
    let raf;
    const step = (t) => {
      const k = Math.min(1, (t - t0) / dur);
      const e = 1 - Math.pow(1 - k, 3);
      setShown(from + (value - from) * e);
      if (k < 1) raf = requestAnimationFrame(step);
      else setPulse((n) => n + 1);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [value]);
  return { shown, pulse };
}

/** 値が変わった回数（最初の表示は数えない）。CSS の動きを再生し直す key に使う */
function useChanges(value) {
  const [n, setN] = useState(0);
  const prev = useRef(value);
  useEffect(() => {
    if (prev.current != null && value != null && prev.current !== value) setN((c) => c + 1);
    prev.current = value;
  }, [value]);
  return n;
}

function Power({ power, isX, isBankara, size }) {
  const { shown, pulse } = useRolling(power);
  if (shown == null) return null;
  return (
    <span key={pulse} className={pulse > 0 ? "disp-power pop" : "disp-power"} style={{ fontSize: size }}>
      {isX ? shown.toFixed(1) : Math.round(shown)}
      {isBankara && <small style={{ fontSize: Math.round(size * 0.42) }}>p</small>}
    </span>
  );
}

export default function DisplayApp() {
  const [s, setS] = useState(null);
  const [settings, setSettings] = useState(null);
  const box = useRef(null);
  const lastSize = useRef("");

  useEffect(() => {
    let alive = true;
    const tick = () => {
      invoke("status").then((st) => alive && setS(st)).catch(() => {});
      invoke("get_settings").then((st) => alive && setSettings(st)).catch(() => {});
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const d = settings?.display ?? { layout: "yoko", bg: "#16161d", font_head: 22, font_power: 72, font_set: 30, outline_px: 0, outline_color: "#000000", text_color: "#ffffff", head_color: "#c8c8d2" };
  // 縁取り: 字の外側にだけ描く（paint-order）。太さは外側に出る分なので 2 倍にする
  const outline = d.outline_px > 0 ? { WebkitTextStroke: `${d.outline_px * 2}px ${d.outline_color}`, paintOrder: "stroke fill" } : {};
  const layout = LAYOUTS[d.layout] ? d.layout : "yoko";

  // 窓の大きさを中身に合わせる（字の大きさや置き方が変わっても余白が同じ）
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const w = Math.ceil(el.scrollWidth + PAD.x * 2);
    const h = Math.ceil(el.scrollHeight + PAD.y * 2);
    const key = `${w}x${h}`;
    if (key === lastSize.current) return;
    lastSize.current = key;
    getCurrentWindow().setSize(new LogicalSize(w, h)).catch(() => {});
  });

  const change = async (patch) => {
    const cur = await invoke("get_settings");
    const next = { ...cur, display: { ...cur.display, ...patch } };
    await invoke("save_settings", { settings: next });
    setSettings(next);
  };

  const onMouseDown = (e) => {
    if (e.button === 0) getCurrentWindow().startDragging().catch(() => {});
  };
  const onContextMenu = async (e) => {
    e.preventDefault();
    const items = [];
    for (const [id, name] of Object.entries(LAYOUTS)) {
      items.push(await CheckMenuItem.new({ text: name, checked: layout === id, action: () => change({ layout: id }) }));
    }
    items.push(await PredefinedMenuItem.new({ item: "Separator" }));
    for (const [c, name] of BGS) {
      items.push(await CheckMenuItem.new({ text: `背景: ${name}`, checked: d.bg === c, action: () => change({ bg: c }) }));
    }
    items.push(await PredefinedMenuItem.new({ item: "Separator" }));
    items.push(await MenuItem.new({ text: "設定を開く", action: () => invoke("open_settings") }));
    items.push(await MenuItem.new({ text: "最小化", action: () => getCurrentWindow().minimize() }));
    items.push(await MenuItem.new({ text: "終了", action: () => getCurrentWindow().close() }));
    const menu = await Menu.new({ items });
    await menu.popup();
  };

  const { mode, rule, xp, udemae, set } = pick(s?.events ?? []);
  const head = [MODE_NAMES[mode] ?? "", RULE_NAMES[rule] ?? ""].filter(Boolean).join(" ");
  const isX = mode === "x";
  const isBankara = mode != null && mode.startsWith("bankara");
  const power = isX ? xp : isBankara ? udemae : null;
  const showSet = set && mode !== "bankara_open" && mode !== "other";
  const setKey = showSet ? `${set.wins}-${set.losses}` : null;
  const setChanges = useChanges(setKey);
  const note = !s ? "" : !s.source ? "N Air または OBS が見つかりません" : s.events.length === 0 ? "まだ試合を認識していません" : "";
  const powerEl = power != null && <Power power={power} isX={isX} isBankara={isBankara} size={d.font_power} />;

  return (
    <div
      className={`disp disp-${layout}`}
      style={{ background: d.bg, color: d.text_color, padding: `${PAD.y}px ${PAD.x}px` }}
      onMouseDown={onMouseDown}
      onContextMenu={onContextMenu}
    >
      {/* 横長は 2 段組み（左: モードとルールの下に勝敗、右: パワー）。縦長は上から順に */}
      <div className="disp-box" ref={box} style={outline}>
        <div className="disp-left">
          <span className="disp-head" style={{ fontSize: d.font_head, color: d.head_color }}>
            {head || " "}
          </span>
          {layout === "tate" && powerEl}
          {showSet && (
            <span key={setChanges} className={setChanges > 0 ? "disp-set pop" : "disp-set"} style={{ fontSize: d.font_set }}>
              {set.wins}勝 {set.losses}敗
            </span>
          )}
          {note && <span className="disp-note">{note}</span>}
        </div>
        {layout === "yoko" && powerEl}
      </div>
    </div>
  );
}
