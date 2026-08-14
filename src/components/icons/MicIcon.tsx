// 统一的简约麦克风图标（细线描边、currentColor 继承文字颜色），
// 替换各处风格不一的 emoji（🎤/🎙️）。
// filled=true 时话筒主体实心填充（用于「生效中」状态指示，如标题栏绿灯）。

export function MicIcon({
  size = 16,
  className,
  filled = false,
}: {
  size?: number;
  className?: string;
  filled?: boolean;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden
    >
      <path
        d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"
        fill={filled ? "currentColor" : "none"}
      />
      <path d="M19 11v1a7 7 0 0 1-14 0v-1" />
      <line x1="12" y1="19" x2="12" y2="22" />
      <line x1="8" y1="22" x2="16" y2="22" />
    </svg>
  );
}
