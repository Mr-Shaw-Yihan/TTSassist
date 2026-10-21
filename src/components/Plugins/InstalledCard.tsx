// 已安装插件卡（竖版，适配阵列网格）：身份行 → 属性徽标行 → 状态一句话 → 主操作 + 弱操作。
// 配置职能不在插件库（语音中心负责）；启用/停用开关由 enabled/onToggle 传入后渲染。

import { useSettingsStore } from "../../stores/settingsStore";
import { isNewer } from "../../utils/version";
import { supportsClone, type PluginCat } from "../../hooks/usePluginInventory";
import type { PluginInfo, PluginIndexEntry } from "../../types";
import type { PluginActions } from "../../hooks/usePluginActions";

export function InstalledCard({
  p,
  cat,
  online,
  actions,
  busy,
  enabled,
  onToggle,
}: {
  p: PluginInfo;
  cat: PluginCat;
  /** 在线索引中的同 id 条目（有新版本时显示更新主操作） */
  online?: PluginIndexEntry;
  actions: PluginActions;
  busy: string | null;
  /** 启用/停用（后端停用能力就绪前不传，开关不渲染） */
  enabled?: boolean;
  onToggle?: (p: PluginInfo, next: boolean) => void;
}) {
  const settings = useSettingsStore((s) => s.settings);

  const isCurrentEngine =
    (cat === "tts" && settings?.tts_engine === p.id) ||
    (cat === "asr" && settings?.asr_plugin === p.id);
  // 必填缺失 → 待配置提示（配置入口在语音中心，此处仅状态展示）
  const missingRequired = (p.config?.fields ?? []).filter(
    (f) => f.required && !(settings?.plugin_config?.[p.id]?.[f.key] ?? "").trim(),
  );
  const hasUpdate = online ? isNewer(online.version, p.version) : false;

  return (
    <div
      className={[
        "flex flex-col rounded-xl border bg-[var(--paper-card)] px-3.5 py-3 shadow-[0_1px_2px_rgba(26,24,22,0.03)]",
        isCurrentEngine && p.loaded ? "border-l-[3px] border-l-[var(--amber-500)]" : "",
        "border-[var(--ink-200)]",
      ].join(" ")}
    >
      {/* 身份行：名称 + 版本 + 状态徽标 + 启停开关 */}
      <div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
        <span className="text-[13px] font-medium text-[var(--ink-900)]">{p.name}</span>
        <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]">
          v{p.version}
        </span>
        {isCurrentEngine && p.loaded ? (
          <span className="rounded-md bg-[var(--amber-200)]/70 px-1.5 py-0.5 text-[10px] font-medium text-[var(--amber-600)]">
            使用中
          </span>
        ) : !p.loaded ? (
          <span
            className="rounded-md bg-[var(--seal)]/10 px-1.5 py-0.5 text-[10px] font-medium text-[var(--seal)]"
            title={p.error ?? undefined}
          >
            故障
          </span>
        ) : missingRequired.length > 0 ? (
          <span
            className="rounded-md border border-dashed border-[var(--amber-600)]/45 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-[var(--amber-600)]"
            title={`待配置：${missingRequired.map((f) => f.label).join("、")}。请在语音中心选中该引擎后填写`}
          >
            ⚙ 待配置
          </span>
        ) : null}
        {enabled !== undefined && onToggle && (
          <button
            role="switch"
            aria-checked={enabled}
            aria-label={enabled ? `停用 ${p.name}` : `启用 ${p.name}`}
            onClick={() => onToggle(p, !enabled)}
            className={[
              "relative ml-auto h-[19px] w-[32px] shrink-0 rounded-full transition-colors",
              enabled ? "bg-[var(--amber-500)]" : "bg-[var(--ink-200)]",
            ].join(" ")}
          >
            <span
              className={[
                "absolute top-[2px] h-[15px] w-[15px] rounded-full bg-[var(--paper-card)] shadow-[0_1px_3px_rgba(26,24,22,0.25)] transition-transform",
                enabled ? "translate-x-[15px]" : "translate-x-[2px]",
              ].join(" ")}
            />
          </button>
        )}
      </div>

      {/* 属性徽标行（名称下方，统一一排） */}
      <div className="mt-1.5 flex flex-wrap gap-1">
        <span className="rounded border border-[var(--ink-200)] px-1.5 py-0.5 text-[9.5px] text-[var(--ink-500)]">
          {p.category === "local" ? "本地离线" : "云端"}
        </span>
        {supportsClone(p.id) && (
          <span className="rounded border border-[var(--ink-200)] px-1.5 py-0.5 text-[9.5px] text-[var(--ink-500)]">
            支持克隆
          </span>
        )}
      </div>

      {/* 描述 */}
      <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-500)]" title={p.description}>
        {p.description || "　"}
      </p>

      {/* 失败原因 */}
      {!p.loaded && p.error && (
        <div className="mt-2 rounded-lg border border-[var(--seal)]/20 bg-[var(--seal)]/5 px-2.5 py-1.5 text-[10.5px] leading-relaxed text-[var(--seal)]">
          {p.error}
        </div>
      )}

      {/* 操作行 */}
      <div className="mt-auto flex flex-wrap items-center gap-0.5 pt-2.5">
        {hasUpdate && online && (
          <button
            onClick={() =>
              void actions.installCandidate({
                id: online.id,
                name: online.name,
                version: online.version,
                description: online.description,
                requirements: online.requirements,
                source: "online",
              })
            }
            disabled={busy !== null}
            className="mr-1 rounded-lg border border-[var(--amber-500)] px-2.5 py-1 text-[11px] font-medium text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-200)]/30 disabled:opacity-40"
          >
            更新至 v{online.version}
          </button>
        )}
        <span className="flex-1" />
        <button
          onClick={() => void actions.openLocation(p)}
          className="rounded-md px-1.5 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-600)]"
        >
          打开位置
        </button>
        <button
          onClick={() => void actions.uninstall(p)}
          disabled={busy !== null}
          className="rounded-md px-1.5 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--seal)]/10 hover:text-[var(--seal)] disabled:opacity-40"
        >
          卸载
        </button>
      </div>
    </div>
  );
}
