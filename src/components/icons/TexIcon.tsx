// 纹理图标系统：「材质填充 + 不封闭断线勾勒」双皮肤图标。
// - 图标主体 fill=var(--tx)：浅色皮肤为哑金磨砂纹理，深色皮肤为紫水晶星点纹理
//   （pattern 定义见下方 TexDefs，随主题经 CSS 变量切换）
// - 轮廓 stroke=var(--ln)：浅色墨色 / 深色白色的断续线条，只勾勒部分轮廓
// 注意：路径样式必须写在 <symbol> 内联 style 上 —— <use> 的影子树里
// 文档 CSS 选择器不可达，但内联 style 的 var() 会随继承解析。
// 按钮内使用时，由 .btn-tex 覆盖 --tx/--ln 让图标变为实心断线形态。

export type TexIconName =
  | "msg"
  | "star"
  | "grid"
  | "gear"
  | "dots"
  | "mic"
  | "play"
  | "send"
  | "copy"
  | "trash";

/** 全局纹理与符号定义：主窗口挂载一次即可（放到 App 根节点） */
export function TexDefs() {
  return (
    <svg width="0" height="0" style={{ position: "absolute" }} aria-hidden>
      <defs>
        {/* 哑金磨砂纹理（参照图一）：上浅下深的浓金黄渐变 + 深色细砂颗粒 + 少量亮砂 */}
        <linearGradient id="goldGrad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#f2e7b3" />
          <stop offset="0.5" stopColor="#e9d07c" />
          <stop offset="1" stopColor="#d2a23e" />
        </linearGradient>
        <filter id="mottleDark" x="0" y="0" width="100%" height="100%">
          <feTurbulence type="fractalNoise" baseFrequency="0.7" numOctaves="4" seed="7" />
          <feColorMatrix values="0 0 0 0 0.55  0 0 0 0 0.40  0 0 0 0 0.10  0 0 0 3.8 -1.5" />
        </filter>
        <filter id="mottleLight" x="0" y="0" width="100%" height="100%">
          <feTurbulence type="fractalNoise" baseFrequency="0.7" numOctaves="4" seed="21" />
          <feColorMatrix values="0 0 0 0 1  0 0 0 0 0.97  0 0 0 0 0.85  0 0 0 3.8 -1.7" />
        </filter>
        <pattern id="goldTex" width="120" height="120" patternUnits="userSpaceOnUse">
          <rect width="120" height="120" fill="url(#goldGrad)" />
          <rect width="120" height="120" filter="url(#mottleDark)" opacity="0.38" />
          <rect width="120" height="120" filter="url(#mottleLight)" opacity="0.55" />
        </pattern>

        {/* 紫水晶磨砂纹理（暗档）：中心稍淡的径向深紫 + 磨砂颗粒 + 白色星点（参考「紫色闪粉」质感） */}
        <radialGradient id="purpGrad" cx="0.5" cy="0.4" r="0.75">
          <stop offset="0" stopColor="#a088cf" />
          <stop offset="0.48" stopColor="#7c5bb0" />
          <stop offset="1" stopColor="#523c85" />
        </radialGradient>
        <filter id="sandPurp" x="0" y="0" width="100%" height="100%">
          <feTurbulence type="fractalNoise" baseFrequency="0.55" numOctaves="3" seed="11" />
          <feColorMatrix values="0 0 0 0 0.16  0 0 0 0 0.10  0 0 0 0 0.28  0 0 0 0.75 0" />
        </filter>
        <pattern id="purpTex" width="64" height="64" patternUnits="userSpaceOnUse">
          <rect width="64" height="64" fill="url(#purpGrad)" />
          <rect width="64" height="64" filter="url(#sandPurp)" opacity="0.5" />
          <g fill="#ffffff" opacity="0.9">
            <circle cx="8" cy="10" r=".6" /><circle cx="21" cy="29" r=".45" />
            <circle cx="37" cy="11" r=".65" /><circle cx="50" cy="26" r=".5" />
            <circle cx="57" cy="47" r=".6" /><circle cx="14" cy="49" r=".55" />
            <circle cx="31" cy="58" r=".45" /><circle cx="46" cy="61" r=".6" />
            <circle cx="60" cy="12" r=".45" /><circle cx="5" cy="36" r=".5" />
            <circle cx="27" cy="5" r=".55" /><circle cx="42" cy="39" r=".4" />
            <circle cx="54" cy="56" r=".45" /><circle cx="11" cy="61" r=".4" />
            <circle cx="35" cy="22" r=".35" /><circle cx="51" cy="39" r=".55" />
          </g>
        </pattern>
      </defs>

      {/* ── 符号库：fill=var(--tx) 纹理主体 / stroke=var(--ln) 不封闭轮廓线 ── */}
      {/* 消息气泡 */}
      <symbol id="ti-msg" viewBox="0 0 24 24">
        <path
          style={{ fill: "var(--tx)" }}
          d="M12 3.2c5 0 9 3.4 9 7.6s-4 7.6-9 7.6c-.9 0-1.8-.1-2.6-.3L5 20.6l.9-3.2C4.1 15.9 3 13.6 3 10.8 3 6.6 7 3.2 12 3.2z"
        />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round", strokeLinejoin: "round" }}
          strokeDasharray="26 7 30 5 22 8"
          d="M20.9 12.4c-.7 3.5-4.3 6-8.9 6-.9 0-1.8-.1-2.6-.3L5 20.6l.9-3.2C4.4 16.2 3.5 14.6 3.2 12.8"
        />
      </symbol>
      {/* 收藏星 */}
      <symbol id="ti-star" viewBox="0 0 24 24">
        <path
          style={{ fill: "var(--tx)" }}
          d="M12 2.8l2.9 5.9 6.5.9-4.7 4.5 1.1 6.4L12 17.5l-5.8 3 1.1-6.4L2.6 9.6l6.5-.9z"
        />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round", strokeLinejoin: "round" }}
          strokeDasharray="9 5 12 6"
          d="M12 2.8l2.9 5.9 6.5.9-4.7 4.5"
        />
      </symbol>
      {/* 插件四宫格 */}
      <symbol id="ti-grid" viewBox="0 0 24 24">
        <rect style={{ fill: "var(--tx)" }} x="3" y="3" width="8" height="8" rx="2.2" />
        <rect style={{ fill: "var(--tx)" }} x="13" y="3" width="8" height="8" rx="2.2" />
        <rect style={{ fill: "var(--tx)" }} x="3" y="13" width="8" height="8" rx="2.2" />
        <rect style={{ fill: "var(--tx)" }} x="13" y="13" width="8" height="8" rx="2.2" />
        <rect
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          pathLength={100} strokeDasharray="22 10 14 12" x="13" y="3" width="8" height="8" rx="2.2"
        />
        <rect
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          pathLength={100} strokeDasharray="16 11 20 13" x="3" y="13" width="8" height="8" rx="2.2"
        />
      </symbol>
      {/* 设置齿轮 */}
      <symbol id="ti-gear" viewBox="0 0 24 24">
        <path
          style={{ fill: "var(--tx)" }}
          d="M19.4 13c.04-.33.06-.66.06-1s-.02-.67-.06-1l2.1-1.65a.5.5 0 0 0 .12-.64l-2-3.46a.5.5 0 0 0-.6-.22l-2.48 1a7.6 7.6 0 0 0-1.72-1L14.5 2.4a.5.5 0 0 0-.5-.4h-4a.5.5 0 0 0-.5.42l-.32 2.63c-.62.26-1.2.6-1.72 1l-2.48-1a.5.5 0 0 0-.6.22l-2 3.46a.5.5 0 0 0 .12.64L4.6 11c-.04.33-.06.66-.06 1s.02.67.06 1l-2.1 1.65a.5.5 0 0 0-.12.64l2 3.46c.13.22.39.31.6.22l2.48-1c.52.4 1.1.74 1.72 1l.32 2.63c.03.24.24.42.5.42h4c.26 0 .47-.18.5-.42l.32-2.63a7.6 7.6 0 0 0 1.72-1l2.48 1c.21.09.47 0 .6-.22l2-3.46a.5.5 0 0 0-.12-.64L19.4 13zM12 15.5A3.5 3.5 0 1 1 12 8.5a3.5 3.5 0 0 1 0 7z"
        />
        <circle
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          pathLength={100} strokeDasharray="26 9 20 10" cx="12" cy="12" r="3.4"
        />
      </symbol>
      {/* 其他（三点） */}
      <symbol id="ti-dots" viewBox="0 0 24 24">
        <circle style={{ fill: "var(--tx)" }} cx="5" cy="12" r="1.9" />
        <circle style={{ fill: "var(--tx)" }} cx="12" cy="12" r="1.9" />
        <circle style={{ fill: "var(--tx)" }} cx="19" cy="12" r="1.9" />
        <circle
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          pathLength={100} strokeDasharray="17 12 15 14" cx="12" cy="12" r="8.4"
        />
      </symbol>
      {/* 麦克风 */}
      <symbol id="ti-mic" viewBox="0 0 24 24">
        <path
          style={{ fill: "var(--tx)" }}
          d="M12 2.5A3.2 3.2 0 0 1 15.2 5.7v5a3.2 3.2 0 0 1-6.4 0v-5A3.2 3.2 0 0 1 12 2.5z"
        />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          strokeDasharray="14 6 9 5" d="M6.4 11.2a5.6 5.6 0 0 0 11.2 0"
        />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          strokeDasharray="3 2.5" d="M12 16.9v3.3"
        />
      </symbol>
      {/* 播放（圆钮） */}
      <symbol id="ti-play" viewBox="0 0 24 24">
        <circle style={{ fill: "var(--tx)" }} cx="12" cy="12" r="8.8" />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round", strokeLinejoin: "round" }}
          strokeDasharray="8 4.5 9 5" d="M9.9 8.6l6.1 3.4-6.1 3.4z"
        />
      </symbol>
      {/* 发送（纸飞机） */}
      <symbol id="ti-send" viewBox="0 0 24 24">
        <path
          style={{ fill: "var(--tx)" }}
          d="M3.4 11.1L20.6 3.4c.5-.22 1 .28.8.78l-7.4 17.5c-.2.5-.92.48-1.1-.03l-2.2-6.3-6.3-2.1c-.5-.17-.53-.9-.03-1.1z"
        />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round", strokeLinejoin: "round" }}
          strokeDasharray="12 6 9 5" d="M20.6 3.4L10.7 15.35"
        />
      </symbol>
      {/* 复制 */}
      <symbol id="ti-copy" viewBox="0 0 24 24">
        <path
          style={{ fill: "var(--tx)" }}
          d="M6 2h9a4 4 0 0 1 .5.03V4H6.5A.5.5 0 0 0 6 4.5V16H4a2 2 0 0 1 0-4V4a2 2 0 0 1 2-2z"
        />
        <rect style={{ fill: "var(--tx)" }} x="8" y="8" width="12" height="12" rx="2" />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          strokeDasharray="10 6 7 5" d="M20 8.9v7.6"
        />
      </symbol>
      {/* 删除 */}
      <symbol id="ti-trash" viewBox="0 0 24 24">
        <path
          style={{ fill: "var(--tx)" }}
          d="M9 3a1 1 0 0 0-1 1v1H4v2h16V5h-4V4a1 1 0 0 0-1-1H9zm-2.6 6l.9 11.2A2 2 0 0 0 9.3 22h5.4a2 2 0 0 0 2-1.8L17.6 9H6.4z"
        />
        <path
          style={{ fill: "none", stroke: "var(--ln)", strokeWidth: "var(--lnw)", strokeLinecap: "round" }}
          strokeDasharray="6 4 5 4" d="M10 12.5v5M14 12.5v5"
        />
      </symbol>
    </svg>
  );
}

/** 纹理图标：主体随皮肤呈现哑金/紫晶质感，轮廓为断续勾线 */
export function TexIcon({
  name,
  size = 16,
  className,
}: {
  name: TexIconName;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      className={className ? ["tex-ic", className].join(" ") : "tex-ic"}
      aria-hidden
    >
      <use href={`#ti-${name}`} />
    </svg>
  );
}
