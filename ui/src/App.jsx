import React, { useState } from "react";
import StatusPage from "./StatusPage.jsx";
import TemplatesPage from "./TemplatesPage.jsx";
import SettingsPage from "./SettingsPage.jsx";
import MaterialsPage from "./MaterialsPage.jsx";
import ManualPage from "./ManualPage.jsx";

const TABS = [
  { id: "manual", name: "手動操作" },
  { id: "settings", name: "設定" },
  { id: "materials", name: "テンプレートの状況" },
  { id: "templates", name: "テンプレートの登録" },
  { id: "status", name: "状態" },
];

export default function App() {
  const [tab, setTab] = useState("manual");
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
      {tab === "manual" && <ManualPage />}
      {tab === "status" && <StatusPage />}
      {tab === "materials" && <MaterialsPage />}
      {tab === "templates" && <TemplatesPage />}
      {tab === "settings" && <SettingsPage />}
    </div>
  );
}
