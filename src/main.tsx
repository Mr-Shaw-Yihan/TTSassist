import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import "./styles/globals.css";

// 启动早期应用主题（先于首帧渲染，避免深色设置下白皮闪烁/残留）
invoke<{ theme?: string }>("get_settings")
  .then((s) => {
    document.documentElement.setAttribute("data-theme", s.theme === "dark" ? "dark" : "light");
  })
  .catch(() => { /* 读设置失败用默认浅色 */ });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);