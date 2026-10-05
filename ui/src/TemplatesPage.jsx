import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const POOL_NAMES = {
  rule_intro: "ルール紹介",
  outcome: "勝敗",
  mode: "モード",
  rule: "ルール",
  power_label: "「Xパワー」の見出し",
  digit: "大きな数字",
  digit_small: "増減の数字",
  matching: "マッチング",
  udemae_title: "精算の見出し",
  digit_gauge: "精算の小さな数字",
  digit_total: "精算の TOTAL の数字",
  progress_label: "進行の「WIN LOSE」",
  menu_x: "メニューの「Xパワー :」",
  menu_udemae: "メニューの「ウデマエ」",
  digit_menu: "メニューの数字",
};

/** 見えたもの（Seen の名前）の日本語 */
const SEEN_NAMES = {
  Matching: "マッチング",
  RuleIntro: "ルール紹介",
  NoContestNotice: "無効試合の札",
  Outcome: "勝敗",
  Header: "結果の帯",
  XPower: "Xパワー",
  Calibrating: "計測中",
  Calibrated: "計測完了",
  Udemae: "ウデマエ",
  UdemaeReset: "昇格",
  Observed: "メニュー",
  Progress: "進行",
};

const seenKind = (seen) => seen.split(/[({ ]/)[0];

/** 同じものが続いた所をまとめる（何も読めなかった所は除く） */
function segments(frames) {
  const out = [];
  frames.forEach((f, i) => {
    const kind = seenKind(f.seen);
    const last = out[out.length - 1];
    if (last && last.kind === kind && last.end === i - 1) last.end = i;
    else if (SEEN_NAMES[kind]) out.push({ kind, start: i, end: i });
  });
  return out;
}

/** ファイルを data URL で読む */
function readFile(file) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(r.error);
    r.readAsDataURL(file);
  });
}

export default function TemplatesPage() {
  const [places, setPlaces] = useState([]);
  const [placeId, setPlaceId] = useState("outcome");
  const [source, setSource] = useState(null);
  const [view, setView] = useState(null);
  const [text, setText] = useState("");
  const [pools, setPools] = useState([]);
  const [msg, setMsg] = useState(null);
  const [gaps, setGaps] = useState([]);
  // 取り込んだ直近の画面と、いま見ている位置
  const [held, setHeld] = useState(null);
  const [heldAt, setHeldAt] = useState(0);

  const place = places.find((p) => p.id === placeId);

  const refreshPools = useCallback(async () => {
    const [p, st] = await Promise.all([invoke("list_templates"), invoke("status")]);
    setPools(p);
    setGaps(st.template_gaps);
  }, []);

  useEffect(() => {
    invoke("places").then(setPlaces);
    refreshPools();
  }, [refreshPools]);

  // 元の絵か場所が変わったら切り出し直す
  const refreshView = useCallback(async () => {
    if (!source) return;
    try {
      const v = await invoke("inspect", { placeId });
      setView(v);
      // 読めない字があれば、手がかりの数字での推測を入れておく（確かめてから登録する）
      if (v.reading) {
        const r = v.reading;
        setText(!r.text.includes("?") ? r.text : r.guess.includes("?") ? "" : r.guess);
      }
    } catch (e) {
      setMsg({ bad: true, text: String(e) });
    }
  }, [source, placeId]);

  useEffect(() => {
    refreshView();
  }, [refreshView]);

  const run = async (f, ok) => {
    try {
      const r = await f();
      setMsg({ bad: false, text: ok(r) });
      await refreshPools();
      await refreshView();
    } catch (e) {
      setMsg({ bad: true, text: String(e) });
    }
  };

  const useLive = () => run(async () => setSource(await invoke("source_from_live")), () => "今の画面を元にした");
  const holdRecent = () =>
    run(
      async () => {
        const list = await invoke("hold_recent");
        setHeld(list);
        if (list.length > 0) setHeldAt(list.length - 1);
        return list.length;
      },
      (n) => (n > 0 ? `直近 ${n} 枚を取り込んだ。バーで戻って選ぶ` : "まだ撮れた画面が無い")
    );

  // バーを動かしたら、少し止まってからその画面を元にする（動かしている間は読み直さない）
  useEffect(() => {
    if (!held || held.length === 0) return;
    const id = setTimeout(() => {
      invoke("source_from_held", { index: heldAt })
        .then(setSource)
        .catch((e) => setMsg({ bad: true, text: String(e) }));
    }, 150);
    return () => clearTimeout(id);
  }, [held, heldAt]);

  const useFile = async (e) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    run(async () => setSource(await invoke("source_from_file", { dataUrl: await readFile(file) })), () => `${file.name} を元にした`);
  };

  return (
    <div className="page templates">
      <p className="small">
        見本はあなたの画面から登録します（ゲームの画面は配る exe に入れないため）。遊んだ後に「直近の画面から選ぶ」で戻り、
        数字や勝敗が映った画面を選んでください（読めた画面は下の印から飛べます）。「今の画面」や、snap・record で残した
        ゲーム穴の画像も使えます。枠をクリックすると、その場所を切り出します。
      </p>
      <div className="toolbar">
        <button onClick={holdRecent}>直近の画面から選ぶ</button>
        <button onClick={useLive}>今の画面を使う</button>
        <label className="file">
          画像ファイルを選ぶ
          <input type="file" accept="image/png,image/jpeg" onChange={useFile} />
        </label>
        {msg && <span className={msg.bad ? "bad" : "good"}>{msg.text}</span>}
      </div>

      {held && held.length > 0 && (
        <div className="seek">
          <div className="seek-bar">
            <button onClick={() => setHeldAt(Math.max(0, heldAt - 1))}>◀</button>
            <input
              type="range"
              min={0}
              max={held.length - 1}
              value={heldAt}
              onChange={(e) => setHeldAt(Number(e.target.value))}
            />
            <button onClick={() => setHeldAt(Math.min(held.length - 1, heldAt + 1))}>▶</button>
            <span className="seek-time">
              {held[heldAt].time}（{heldAt + 1}/{held.length}）{SEEN_NAMES[seenKind(held[heldAt].seen)] ?? ""}
            </span>
          </div>
          <div className="seek-marks">
            {segments(held).map((g) => (
              <button
                key={g.start}
                className={heldAt >= g.start && heldAt <= g.end ? "on" : ""}
                title={held[g.start].seen}
                onClick={() => setHeldAt(Math.floor((g.start + g.end) / 2))}
              >
                {held[g.start].time.slice(0, 8)} {SEEN_NAMES[g.kind]}
              </button>
            ))}
          </div>
        </div>
      )}

      {gaps.length > 0 && (
        <div className="gaps small">
          足りない見本: {gaps.join(" / ")}
        </div>
      )}

      <div className="row">
        <div className="col source">
          {source ? (
            <div className="frame">
              <img src={source} alt="元の絵" />
              {places.map((p) => (
                <div
                  key={p.id}
                  className={p.id === placeId ? "roi on" : "roi"}
                  title={p.name}
                  style={{
                    left: `${(p.x / 1024) * 100}%`,
                    top: `${(p.y / 576) * 100}%`,
                    width: `${(p.w / 1024) * 100}%`,
                    height: `${(p.h / 576) * 100}%`,
                  }}
                  onClick={() => setPlaceId(p.id)}
                />
              ))}
            </div>
          ) : (
            <div className="noframe">元の絵を選んでください</div>
          )}
          {view && (
            <div className="seen">
              この絵ぜんたいの読み: <b>{view.seen}</b>
              {view.notes.map((n, i) => (
                <div key={i} className="note">
                  {n}
                </div>
              ))}
            </div>
          )}
        </div>

        <div className="col inspect">
          <select value={placeId} onChange={(e) => setPlaceId(e.target.value)}>
            {places.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          {view && place && (
            <>
              <div className="cut">
                <img src={view.crop} alt="切り出し" />
                <img src={view.binary} alt="白黒" className="bin" />
              </div>
              {!place.glyphs && (
                <>
                  <table className="scores">
                    <tbody>
                      {view.scores.length === 0 && (
                        <tr>
                          <td className="small">この場所の見本はまだ無い</td>
                        </tr>
                      )}
                      {view.scores.map((s) => (
                        <tr key={s.label}>
                          <th>{place.labels.find((l) => l.id === s.label)?.name ?? s.label}</th>
                          <td>一致度 {s.score.toFixed(3)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <div className="labels">
                    この絵を
                    {place.labels.map((l) => (
                      <button
                        key={l.id}
                        onClick={() =>
                          run(() => invoke("register_label", { placeId, label: l.id }), () => `「${l.name}」の見本を足した`)
                        }
                      >
                        {l.name}
                      </button>
                    ))}
                    として登録
                  </div>
                </>
              )}
              {place.glyphs && (
                <>
                  <div className="glyphs">
                    {view.glyphs.map((g, i) =>
                      g.image ? <img key={i} src={g.image} alt="" /> : <span key={i} className="dot">{g.mark}</span>
                    )}
                  </div>
                  {view.reading && (
                    <div>
                      読み: <b>{view.reading.text}</b>{" "}
                      {view.reading.guess !== view.reading.text && (
                        <span className="small">（手がかりの数字での推測: {view.reading.guess}。確かめてから登録）</span>
                      )}{" "}
                      <span className="small">
                        1 文字ずつの一致度 {view.reading.chars.map(([c, v]) => `${c}:${v.toFixed(2)}`).join(" ")}
                      </span>
                    </div>
                  )}
                  <div className="labels">
                    見えている数字
                    <input value={text} onChange={(e) => setText(e.target.value)} placeholder="2194.6 / +62.2" />
                    <button
                      disabled={!text}
                      onClick={() =>
                        run(() => invoke("register_glyphs", { placeId, text }), (n) => `${n} 文字の見本を足した`)
                      }
                    >
                      1 文字ずつ登録
                    </button>
                  </div>
                </>
              )}
            </>
          )}
        </div>
      </div>

      <h3>登録した見本</h3>
      {pools.map((p) => (
        <div key={p.pool} className="pool">
          <div className="pool-name">
            {POOL_NAMES[p.pool] ?? p.pool}（{p.templates.length}）
          </div>
          <div className="thumbs">
            {p.templates.map((t) => (
              <div key={t.id} className={t.auto ? "thumb auto" : "thumb"} title={t.auto ? `${t.id}（自動で足した）` : t.id}>
                <img src={t.image} alt={t.label} />
                <span>
                  {t.label}
                  {t.auto && <em>自動</em>}
                </span>
                <button
                  className="del"
                  onClick={() => run(() => invoke("delete_template", { pool: p.pool, id: t.id }), () => "消した")}
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
