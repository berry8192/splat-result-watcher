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
};

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

  const place = places.find((p) => p.id === placeId);

  const refreshPools = useCallback(() => invoke("list_templates").then(setPools), []);

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
      if (v.reading) setText(v.reading.text.includes("?") ? "" : v.reading.text);
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
  const useFile = async (e) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    run(async () => setSource(await invoke("source_from_file", { dataUrl: await readFile(file) })), () => `${file.name} を元にした`);
  };

  return (
    <div className="page templates">
      <p className="small">
        見本はあなたの画面から登録します（ゲームの画面は配る exe に入れないため）。元の絵は「今の画面」か、snap・record で残した
        ゲーム穴の画像（samples/ の中）を選びます。枠をクリックすると、その場所を切り出します。
      </p>
      <div className="toolbar">
        <button onClick={useLive}>今の画面を使う</button>
        <label className="file">
          画像ファイルを選ぶ
          <input type="file" accept="image/png,image/jpeg" onChange={useFile} />
        </label>
        {msg && <span className={msg.bad ? "bad" : "good"}>{msg.text}</span>}
      </div>

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
              <div key={t.id} className="thumb" title={t.id}>
                <img src={t.image} alt={t.label} />
                <span>{t.label}</span>
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
