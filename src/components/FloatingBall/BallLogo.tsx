// 主窗标题栏左上角 logo：悬浮球的「休眠态」静态占位（sleeping 样式）。
// 双态：球在外时半透明弱化（本体已外出）；点击 = 放出/收回悬浮球
// （显隐走 set_floating_ball_enabled，与菜单「关闭悬浮球」同一条后端逻辑）。

import { useSettingsStore } from "../../stores/settingsStore";
import { setFloatingBallEnabled } from "../../services/invoke";

export function BallLogo() {
  const ballOut = useSettingsStore((s) => s.settings?.floating_ball_enabled ?? false);
  const skin = useSettingsStore((s) => s.settings?.floating_ball_skin ?? "ink");
  const white = skin === "white";

  return (
    <button
      type="button"
      title={ballOut ? "收回悬浮球" : "放出悬浮球"}
      aria-label={ballOut ? "收回悬浮球" : "放出悬浮球"}
      onClick={() => {
        void setFloatingBallEnabled(!ballOut).catch((e) => {
          window.alert(`切换悬浮球失败：${e}`);
        });
      }}
      className={[
        "flex h-6 w-6 shrink-0 items-center justify-center rounded-full transition-all",
        "hover:scale-110",
        ballOut ? "opacity-40" : "opacity-100",
      ].join(" ")}
    >
      {/* sleeping 静态占位：皮肤同步（墨黑体/素白体）+ 闭眼弧线 */}
      <svg viewBox="0 0 24 24" className="h-6 w-6" aria-hidden>
        <circle
          cx="12"
          cy="12"
          r="10"
          fill={white ? "#f7f4ec" : "var(--ink-900)"}
          stroke={white ? "var(--ink-200)" : "none"}
          strokeWidth="1"
        />
        <path
          d="M7.5 13.5q1.7 1.8 3.4 0"
          stroke={white ? "#1a1816" : "var(--paper)"}
          strokeWidth="1.4"
          fill="none"
          strokeLinecap="round"
        />
        <path
          d="M13.1 13.5q1.7 1.8 3.4 0"
          stroke={white ? "#1a1816" : "var(--paper)"}
          strokeWidth="1.4"
          fill="none"
          strokeLinecap="round"
        />
      </svg>
    </button>
  );
}
