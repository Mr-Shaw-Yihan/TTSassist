import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import { applyTheme, persistTheme, resolveInitialTheme, THEME_STORAGE_KEY } from "./utils/theme";
import "./styles/globals.css";

// 皮肤兜底：正常路径由 index.html 的内联同步脚本定好首帧主题，这里再从 localStorage 应用一次，
// 覆盖不走 index.html 的入口（如 dev-preview.html）。
applyTheme(resolveInitialTheme(localStorage.getItem(THEME_STORAGE_KEY)));

// 把后端真实皮肤记回 localStorage，下次启动首帧即可正确（老版本升级上来时副本为空）
invoke<{ theme?: string }>("get_settings")
  .then((s) => {
    const theme = resolveInitialTheme(s.theme);
    persistTheme(theme);
    applyTheme(theme);
  })
  .catch(() => { /* 读设置失败用默认浅色 */ });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);