// 首帧皮肤（B2）。
// 为什么需要它：真实主题存在后端 settings.json，要异步加载后才能写 <html data-theme>，
// 而 CSS 的浅色变量挂在 :root 上 —— 于是夜间用户每次启动都会先渲染一帧日间皮肤。
// 解法是把皮肤镜像一份到 localStorage（同步可读），由 index.html 的内联同步脚本在首帧前应用。
// index.html 里的脚本是这里规则的副本（它在模块图之前执行，无法 import），
// theme.test.ts 会钉住键名与那段脚本，改一侧必须同步另一侧。

export type Theme = "dark" | "light";

export const THEME_STORAGE_KEY = "va-theme";

/** 由持久化原始串判定首帧皮肤：只有明确是 dark 才算夜间，缺失/坏数据/未知取值一律回落 light */
export function resolveInitialTheme(raw: string | null | undefined): Theme {
  const value = raw?.trim();
  if (!value) return "light";
  if (value === "dark") return "dark";
  if (value.startsWith("{")) {
    try {
      const parsed: unknown = JSON.parse(value);
      return (parsed as { theme?: unknown }).theme === "dark" ? "dark" : "light";
    } catch {
      return "light";
    }
  }
  return "light";
}

/** 记住皮肤，供下次启动的首帧脚本读取；存储不可用（隐身模式等）时静默跳过 */
export function persistTheme(theme: Theme): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    /* 忽略：拿不到 localStorage 只是退化成异步换肤，不影响功能 */
  }
}

/** 立刻换肤 */
export function applyTheme(theme: Theme): void {
  document.documentElement.setAttribute("data-theme", theme);
}
