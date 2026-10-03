import React, { useState } from "react";
import StatusPage from "./StatusPage.jsx";
import TemplatesPage from "./TemplatesPage.jsx";
import SettingsPage from "./SettingsPage.jsx";

const TABS = [
  { id: "status", name: "状態" },
  { id: "templates", name: "見本の登録" },
  { id: "settings", name: "設定" },
];

export default function App() {
  const [tab, setTab] = useState("status");
  return (
    <div className="app">
      <nav className="tabs">
        <span className="brand">splat-result-watcher</span>
        {TABS.map((t) => (
          <button key={t.id} className={tab === t.id ? "tab on" : "tab"} onClick={() => setTab(t.id)}>
            {t.name}
          </button>
        ))}
      </nav>
      {/* 状態のページは裏でも動かし続ける必要がないので、開いているものだけ描く */}
      {tab === "status" && <StatusPage />}
      {tab === "templates" && <TemplatesPage />}
      {tab === "settings" && <SettingsPage />}
    </div>
  );
}
