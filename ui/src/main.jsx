import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App.jsx";
import DisplayApp from "./DisplayApp.jsx";
import "./App.css";

// 窓は 2 つ: 勝敗などを見せる小さな窓（既定）と、設定・見本の窓（#settings）
const settings = window.location.hash === "#settings";
document.body.classList.add(settings ? "settings-window" : "display-window");

createRoot(document.getElementById("root")).render(settings ? <App /> : <DisplayApp />);
