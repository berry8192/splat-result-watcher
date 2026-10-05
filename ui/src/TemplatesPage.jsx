import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const POOL_NAMES = {
  rule_intro: "ルール紹介",
  outcome: "勝敗",
  mode: "モード",
  rule: "ルール",
  power_label: "「Xパワー」のラベル",
  digit: "大きな数字",
  digit_small: "増減の数字",
  matching: "マッチング",
  udemae_title: "精算画面の見出し",
  digit_gauge: "精算画面の小さな数字",
  digit_total: "精算画面の TOTAL の数字",
  progress_label: "セット進行の「WIN LOSE」",
  menu_x: "メニューの「Xパワー :」",
  menu_udemae: "メニューの「ウデマエ」",
  digit_menu: "メニューの数字",
};

/** 見えたもの（Seen の名前）の日本語 */
const SEEN_NAMES = {
  Matching: "マッチング",
  RuleIntro: "ルール紹介",
  NoContestNotice: "無効試合の表示",
  Outcome: "勝敗",
  Header: "個人リザルトの見出し",
  XPower: "Xパワー",
  Calibrating: "計測中",
  Calibrated: "計測完了",
  Udemae: "ウデマエ",
  UdemaeReset: "昇格",
  Observed: "メニュー",
  Progress: "セット進行",
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
      // 認識できない文字があれば、参照フォントによる推定を入れておく（確認してから登録する）
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

  const useLive = () => run(async () => setSource(await invoke("source_from_live")), () => "現在のフレームを元画像にしました");
  const holdRecent = () =>
    run(
      async () => {
        const list = await invoke("hold_recent");
        setHeld(list);
        if (list.length > 0) setHeldAt(list.length - 1);
        return list.length;
      },
      (n) => (n > 0 ? `最近の ${n} フレームを取り込みました。バーで戻って選択してください` : "まだキャプチャしたフレームがありません")
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
    run(async () => setSource(await invoke("source_from_file", { dataUrl: await readFile(file) })), () => `${file.name} を元画像にしました`);
  };

  return (
    <div className="page templates">
      <details className="sec">
        <summary>使い方</summary>
        <p className="small">
        テンプレートはご自身の画面から登録します（ゲーム画面の画像は配布物に含めないため）。プレイ後に「最近のフレームから選ぶ」で遡り、
        数字や勝敗が表示されたフレームを選択してください（認識できたフレームには下の目印から移動できます）。「現在のフレーム」や、
        snap・record で保存したゲーム画面の画像も使用できます。枠をクリックすると、その領域を切り抜きます。
        </p>
      </details>
      <div className="toolbar">
        <button onClick={holdRecent}>最近のフレームから選ぶ</button>
        <button onClick={useLive}>現在のフレームを使う</button>
        <label className="file">
          画像ファイルを開く
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
          不足しているテンプレート: {gaps.join(" / ")}
        </div>
      )}

      <div className="row">
        <div className="col source">
          {source ? (
            <div className="frame">
              <img src={source} alt="元画像" />
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
            <div className="noframe">元画像を選択してください</div>
          )}
          {view && (
            <div className="seen">
              この画像の認識結果: <b>{view.seen}</b>
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
                <img src={view.crop} alt="切り抜き" />
                <img src={view.binary} alt="二値化" className="bin" />
              </div>
              {!place.glyphs && (
                <>
                  <table className="scores">
                    <tbody>
                      {view.scores.length === 0 && (
                        <tr>
                          <td className="small">この領域のテンプレートはまだありません</td>
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
                    この画像を
                    {place.labels.map((l) => (
                      <button
                        key={l.id}
                        onClick={() =>
                          run(() => invoke("register_label", { placeId, label: l.id }), () => `「${l.name}」のテンプレートを追加しました`)
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
                      認識結果: <b>{view.reading.text}</b>{" "}
                      {view.reading.guess !== view.reading.text && (
                        <span className="small">（参照フォントによる推定: {view.reading.guess}。確認してから登録してください）</span>
                      )}{" "}
                      <span className="small">
                        文字ごとの一致度 {view.reading.chars.map(([c, v]) => `${c}:${v.toFixed(2)}`).join(" ")}
                      </span>
                    </div>
                  )}
                  <div className="labels">
                    表示されている数字
                    <input value={text} onChange={(e) => setText(e.target.value)} placeholder="2194.6 / +62.2" />
                    <button
                      disabled={!text}
                      onClick={() =>
                        run(() => invoke("register_glyphs", { placeId, text }), (n) => `${n} 文字のテンプレートを追加しました`)
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

      <h3>登録済みのテンプレート</h3>
      {pools.map((p) => (
        <div key={p.pool} className="pool">
          <div className="pool-name">
            {POOL_NAMES[p.pool] ?? p.pool}（{p.templates.length}）
          </div>
          <div className="thumbs">
            {p.templates.map((t) => (
              <div key={t.id} className={t.auto ? "thumb auto" : "thumb"} title={t.auto ? `${t.id}（自動登録）` : t.id}>
                <img src={t.image} alt={t.label} />
                <span>
                  {t.label}
                  {t.auto && <em>自動</em>}
                </span>
                <button
                  className="del"
                  onClick={() => run(() => invoke("delete_template", { pool: p.pool, id: t.id }), () => "削除しました")}
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
