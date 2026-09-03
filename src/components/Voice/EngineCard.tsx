// 同构引擎卡：语音中心内 TTS 与 ASR 引擎统一以本卡呈现（头部标识 + 卡内控件）。
// 仅提供视觉骨架与身份徽标，具体配置控件由各面板作为 children 传入，逻辑不外溢。
// purpose 为「按用途选引擎」预留（如 voice_input / subtitle）；当前恒为单用途。

export type EngineKind = "tts" | "asr";

/** 云 / 本地 类别徽标文案（category 来自 PluginInfo；内置引擎由调用方显式传） */
function categoryLabel(category?: string): string | null {
  if (category === "local") return "本地";
  if (category === "remote") return "云端";
  return null;
}

export function EngineCard({
  kind,
  name,
  version,
  category,
  loaded = true,
  error,
  purpose,
  badge,
  children,
}: {
  kind: EngineKind;
  name: string;
  version?: string;
  /** 引擎类别："local" 本地离线 / "remote" 联网；未知则不显示徽标 */
  category?: string;
  /** 是否加载成功可用 */
  loaded?: boolean;
  /** 加载失败原因（loaded=false 时展示） */
  error?: string | null;
  /** 用途标识（为未来「按用途选引擎」预留，当前不影响渲染） */
  purpose?: "voice_input" | "subtitle" | "synthesis";
  /** 右侧附加徽标（如「默认」「使用中」） */
  badge?: React.ReactNode;
  children?: React.ReactNode;
}) {
  const cat = categoryLabel(category);
  return (
    <div
      data-purpose={purpose}
      className="rounded-2xl border border-[var(--ink-200)] bg-[var(--paper-card)] shadow-[0_1px_2px_rgba(26,24,22,0.04)]"
    >
      <div className="flex items-center gap-2 px-4 py-3">
        <span
          className={[
            "shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-medium",
            kind === "asr"
              ? "bg-[var(--ink-100)] text-[var(--ink-500)]"
              : "bg-[var(--amber-200)]/50 text-[var(--amber-600)]",
          ].join(" ")}
        >
          {kind === "asr" ? "识别" : "合成"}
        </span>
        <span className="min-w-0 truncate font-display text-[15px] font-semibold tracking-wide text-[var(--ink-900)]">{name}</span>
        {version && <span className="shrink-0 font-mono text-[10px] text-[var(--ink-300)]">v{version}</span>}
        {cat && (
          <span className="shrink-0 rounded-md border border-[var(--ink-200)] px-1.5 py-0.5 text-[10px] text-[var(--ink-500)]">
            {cat}
          </span>
        )}
        {!loaded && (
          <span className="shrink-0 rounded-md bg-[var(--seal)]/10 px-1.5 py-0.5 text-[10px] font-medium text-[var(--seal)]">
            未就绪
          </span>
        )}
        <span className="flex-1" />
        {badge}
      </div>
      {error && (
        <p className="px-4 pb-3 -mt-1 text-[11px] leading-relaxed text-[var(--seal)]">加载失败：{error}</p>
      )}
      {children && (
        <div className="space-y-4 border-t border-[var(--ink-200)]/70 px-4 py-3.5">{children}</div>
      )}
    </div>
  );
}
