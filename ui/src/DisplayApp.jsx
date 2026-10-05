import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Menu, MenuItem, CheckMenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";

// 表示ウィンドウ: 配信ソフトのウィンドウキャプチャで配信に載せる前提の、字だけの小さな窓。
// 枠は無く、左ドラッグで動かし、右クリックのメニュー（OS のもの。窓が小さいので窓の中には描かない）でレイアウト・背景を変える。
// レイアウト・背景・文字サイズ・色は settings.json の `display` に入り、設定の窓と共有する。窓の大きさは中身に合わせる。

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
const DEFAULT_DISPLAY = {
  layout: "yoko",
  bg: "#16161d",
  font_head: 22,
  font_power: 72,
  font_set: 30,
  outline_px: 0,
  outline_color: "#000000",
  x_color: "#2bd9c4",
  bankara_color: "#ff7a2e",
  power_color: "#ffffff",
  set_color: "#f3ea6a",
  head_color: "#8fc5ff",
};

/** 出来事（新しい順）から、見せるものを拾う。それぞれ一番新しいものだけ。
 * `lobby`（ロビーで選んでいるモードとルール）と `manual`（手で直した値）も見る */
export function pick(events) {
  let mode = null;
  let rule = null;
  let xp = null;
  let udemae = null;
  let set = null;
  let ended = null; // セットを終わらせたパワーの変動（match_id）と、その試合の勝敗
  for (const e of events) {
    if (mode == null && (e.type === "result" || e.type === "battle_started" || e.type === "lobby") && e.mode) mode = e.mode;
    if (mode == null && e.type === "observed") mode = e.kind === "x" ? "x" : "bankara";
    if (rule == null && e.rule && (e.type === "result" || e.type === "battle_started" || e.type === "observed" || e.type === "lobby")) rule = e.rule;
    if (e.type === "power" && !e.calibrating && e.after != null) {
      if (e.kind === "x" && xp == null) xp = e.after;
      if (e.kind === "udemae" && udemae == null) udemae = e.after;
    }
    if ((e.type === "observed" || e.type === "manual") && e.value != null) {
      if (e.kind === "x" && xp == null) xp = e.value;
      if (e.kind === "udemae" && udemae == null) udemae = e.value;
    }
    if (set == null && e.wins != null && e.losses != null && (e.type === "set_progress" || e.type === "observed" || e.type === "manual")) {
      set = { wins: e.wins, losses: e.losses };
    }
    // パワーの変動が出たらセットは終わっている（3 勝目・3 敗目の後は進行の画面が出ない）。次のセットは 0 勝 0 敗から
    if (set == null && e.type === "power" && !e.calibrating && e.before != null) {
      set = { wins: 0, losses: 0 };
      ended = { match_id: e.match_id };
    }
    if (ended && ended.outcome == null && e.type === "result" && e.match_id === ended.match_id) ended.outcome = e.outcome;
    // 終わったセットの、最後の試合の前の勝敗（これに最後の勝ち負けを足したものを、変動の間だけ見せる）
    if (ended && ended.prev == null && e.type === "set_progress" && e.match_id !== ended.match_id) {
      ended.prev = { wins: e.wins, losses: e.losses };
    }
  }
  if (set && ended?.outcome && ended.prev) {
    const win = ended.outcome === "win";
    set.final = { wins: ended.prev.wins + (win ? 1 : 0), losses: ended.prev.losses + (win ? 0 : 1) };
  }
  return { mode, rule, xp, udemae, set };
}

/** 値が変わったら、前の値から新しい値へ数え上げる（ドラムロール）。上がるときは 4 秒かけ、下がるときは 1.6 秒。
 * 動いている間は `rolling` */
function useRolling(value) {
  const [shown, setShown] = useState(value);
  const [rolling, setRolling] = useState(false);
  const prev = useRef(value);
  useEffect(() => {
    const from = prev.current;
    prev.current = value;
    if (value == null || from == null || from === value) {
      setShown(value);
      setRolling(false);
      return;
    }
    const t0 = performance.now();
    const dur = value > from ? 4000 : 1600;
    let raf;
    setRolling(true);
    const step = (t) => {
      const k = Math.min(1, (t - t0) / dur);
      const e = 1 - Math.pow(1 - k, 3);
      setShown(from + (value - from) * e);
      if (k < 1) raf = requestAnimationFrame(step);
      else setRolling(false);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [value]);
  return { shown, rolling };
}

function Power({ shown, isX, isBankara, size, color }) {
  if (shown == null) return null;
  return (
    <span className="disp-power" style={{ fontSize: size, color }}>
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

  const d = { ...DEFAULT_DISPLAY, ...(settings?.display ?? {}) };
  // 縁取り: 字の外側にだけ描く（paint-order）。太さは外側に出る分なので 2 倍にする
  const outline = d.outline_px > 0 ? { WebkitTextStroke: `${d.outline_px * 2}px ${d.outline_color}`, paintOrder: "stroke fill" } : {};
  const layout = LAYOUTS[d.layout] ? d.layout : "yoko";

  // 窓の大きさを中身に合わせる（字の大きさやレイアウトが変わっても余白が同じ）
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

  const events = s?.events ?? [];
  const { mode, rule, xp, udemae, set } = pick(events);
  const modeName = MODE_NAMES[mode] ?? "";
  const ruleName = RULE_NAMES[rule] ?? "";
  const isX = mode === "x";
  const isBankara = mode != null && mode.startsWith("bankara");
  const power = isX ? xp : isBankara ? udemae : null;
  const { shown, rolling } = useRolling(power);
  // パワーが動いている間は、終わったセットの勝敗（3 勝目・3 敗目まで入れたもの）を見せ、止まったら 0 勝 0 敗に
  const setShown = set?.final && rolling ? set.final : set;
  const showSet = setShown && mode !== "bankara_open" && mode !== "other";
  const note = !s ? "" : !s.source ? "N Air または OBS が見つかりません" : events.length === 0 ? "まだ試合を認識していません" : "";
  const powerEl = shown != null && <Power shown={shown} isX={isX} isBankara={isBankara} size={d.font_power} color={d.power_color} />;

  return (
    <div
      className={`disp disp-${layout}`}
      style={{ background: d.bg, color: d.power_color, padding: `${PAD.y}px ${PAD.x}px` }}
      onMouseDown={onMouseDown}
      onContextMenu={onContextMenu}
    >
      {/* 横長は 2 段組み（左: モードとルールの下に勝敗、右: パワー）。縦長は上から順に */}
      <div className="disp-box" ref={box} style={outline}>
        <div className="disp-left">
          <span className="disp-head" style={{ fontSize: d.font_head, color: d.head_color }}>
            {modeName && <span style={{ color: isX ? d.x_color : isBankara ? d.bankara_color : d.head_color }}>{modeName} </span>}
            {ruleName || (modeName ? "" : " ")}
          </span>
          {layout === "tate" && powerEl}
          {showSet && (
            <span className="disp-set" style={{ fontSize: d.font_set, color: d.set_color }}>
              {setShown.wins}勝 {setShown.losses}敗
            </span>
          )}
          {note && <span className="disp-note">{note}</span>}
        </div>
        {layout === "yoko" && powerEl}
      </div>
    </div>
  );
}
