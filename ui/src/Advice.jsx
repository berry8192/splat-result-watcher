import React from "react";

/** アプリからの知らせ（入力を促す・読み取りの確度が低い）。字だけで並べる */
export default function Advice({ list }) {
  if (!list || list.length === 0) return null;
  return (
    <div className="advice">
      {list.map((t) => (
        <div key={t}>{t}</div>
      ))}
    </div>
  );
}
