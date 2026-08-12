// Genie 离线资源包获取渠道：百度网盘（点击跳浏览器）+ QQ 群（点击复制群号）
// 复制反馈与设置页「关于」一致：1.5 秒「已复制」浮层，不弹 alert。

import { useId } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

const PAN_URL =
  "https://pan.baidu.com/s/5ZS8XXVIsAJY3ubQggnV1ow#list/path=%2Fsharelink3101293518-784515448752370%2F%E7%94%B5%E5%AD%90%E5%A3%B0%E5%B8%A6%2F%E8%B5%84%E6%BA%90%E5%8C%85&parentPath=%2Fsharelink3101293518-784515448752370";
export const QQ_GROUP = "690907648";

/** 资源包下载渠道一行：百度网盘链接 + QQ 群号（点击复制） */
export function ResourcePackLinks({ className = "" }: { className?: string }) {
  const tipId = useId();

  async function copyGroup() {
    try {
      await navigator.clipboard.writeText(QQ_GROUP);
    } catch {
      /* 复制失败时至少展示群号供手动输入 */
    }
    const el = document.getElementById(tipId);
    if (el) {
      el.style.opacity = "1";
      setTimeout(() => (el.style.opacity = "0"), 1500);
    }
  }

  return (
    <span className={className}>
      <button
        onClick={() =>
          openUrl(PAN_URL).catch(() => window.alert("打开网盘失败，请手动复制地址到浏览器"))
        }
        className="font-medium text-[var(--amber-600)] underline decoration-dotted underline-offset-2 hover:opacity-80"
        title="点击在浏览器打开百度网盘下载页"
      >
        百度网盘下载
      </button>
      <span>，或加入 </span>
      <button
        onClick={() => void copyGroup()}
        className="relative font-mono font-medium text-[var(--amber-600)] underline decoration-dotted underline-offset-2 hover:opacity-80"
        title="点击复制 QQ 群号"
      >
        QQ 群 {QQ_GROUP}
        <span
          id={tipId}
          className="pointer-events-none absolute -top-6 left-1/2 -translate-x-1/2 rounded bg-[var(--ink-700)] px-1.5 py-0.5 text-[10px] text-[var(--paper)] opacity-0 transition-opacity"
        >
          群号已复制
        </span>
      </button>
    </span>
  );
}
