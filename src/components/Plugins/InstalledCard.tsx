// 已安装插件卡（竖版，适配阵列网格）：身份行 → 属性徽标行 → 状态一句话 → 主操作 + 弱操作。
// 配置职能不在插件库（语音中心负责）；启用/停用开关由 enabled/onToggle 传入后渲染。

import { useSettingsStore } from "../../stores/settingsStore";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import { PluginSetupPanel } from "./PluginSetupPanel";
import { ResourcePackLinks } from "./ResourcePackLinks";
import { importResourcePackFlow } from "../../services/invoke";
import { isNewer } from "../../utils/version";
import { supportsClone, type PluginCat } from "../../hooks/usePluginInventory";
import { toast } from "../common/Toast";
import type { PluginInfo, PluginIndexEntry } from "../../types";
import type { PluginActions } from "../../hooks/usePluginActions";

export function InstalledCard({
  p,
  cat,
  online,
  actions,
  busy,
  envPickId,
  setEnvPickId,
  onRefresh,
  enabled,
  onToggle,
}: {
  p: PluginInfo;
  cat: PluginCat;
  /** 在线索引中的同 id 条目（有新版本时显示更新主操作） */
  online?: PluginIndexEntry;
  actions: PluginActions;
  busy: string | null;
  /** 正在展示「安装方式二选一」面板的插件 id */
  envPickId: string | null;
  setEnvPickId: (id: string | null) => void;
  onRefresh: () => void;
  /** 启用/停用（后端停用能力就绪前不传，开关不渲染） */
  enabled?: boolean;
  onToggle?: (p: PluginInfo, next: boolean) => void;
}) {
  const settings = useSettingsStore((s) => s.settings);
  const task = usePluginTaskStore((s) => s.task);
  const startEnv = usePluginTaskStore((s) => s.startEnv);
  const taskRunning = task?.status === "running";

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
        {p.has_setup && (
          <span className="rounded bg-[var(--ink-100)]/70 px-1.5 py-0.5 text-[9.5px] text-[var(--ink-500)]">
            需下载环境
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

      {/* 本地引擎环境安装区（环境安装属插件库职责） */}
      {p.loaded && p.has_setup && (
        <div className="mt-2">
          {task?.pluginId === p.id ? (
            <PluginSetupPanel pluginId={p.id} onClosed={onRefresh} />
          ) : p.setup_status?.ready ? (
            <div className="rounded-lg border border-sky-600/25 bg-sky-600/5 px-2.5 py-1.5 text-[10.5px] text-sky-700">
              ✓ 运行环境就绪 · 音色 {p.setup_status.voices.length} 个（管理在语音中心）
            </div>
          ) : envPickId === p.id ? (
            <div className="rounded-lg border border-sky-600/25 bg-sky-600/5 px-2.5 py-2">
              <div className="mb-1.5 text-[10.5px] font-medium text-sky-800">选择安装方式</div>
              <div className="grid grid-cols-1 gap-1.5">
                <button
                  onClick={() => {
                    setEnvPickId(null);
                    startEnv(p.id, p.name).catch(() => { /* 错误已在 store */ });
                  }}
                  disabled={taskRunning || busy !== null}
                  className="rounded-md border border-sky-600/40 px-2.5 py-1.5 text-left text-[10.5px] font-medium text-sky-700 transition-colors hover:bg-sky-600/10 disabled:opacity-40"
                >
                  在线下载（HuggingFace，国内需先开代理）
                </button>
                <button
                  onClick={async () => {
                    setEnvPickId(null);
                    try {
                      const done = await importResourcePackFlow(p.id);
                      if (done) onRefresh();
                    } catch (e) {
                      toast(`导入资源包失败：${e}`, "err");
                    }
                  }}
                  disabled={taskRunning || busy !== null}
                  className="rounded-md border border-[var(--amber-500)]/60 px-2.5 py-1.5 text-left text-[10.5px] font-medium text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-500)]/10 disabled:opacity-40"
                >
                  导入离线资源包（无需联网）
                </button>
              </div>
              <div className="mt-1.5 flex items-center justify-between gap-2">
                <span className="text-[9.5px] leading-relaxed text-[var(--ink-300)]">
                  {p.id === "genie-tts" ? (
                    <>
                      资源包获取：<ResourcePackLinks />
                    </>
                  ) : (
                    "离线资源包请联系该插件提供方获取"
                  )}
                </span>
                <button
                  onClick={() => setEnvPickId(null)}
                  className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[10.5px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
                >
                  取消
                </button>
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-2 rounded-lg border border-sky-600/25 bg-sky-600/5 px-2.5 py-1.5">
              <span
                className="min-w-0 flex-1 truncate text-[10.5px] text-sky-800"
                title={p.setup_status?.summary ?? ""}
              >
                {p.setup_status?.summary ?? "运行环境未安装"}
              </span>
              <button
                onClick={() => setEnvPickId(p.id)}
                disabled={taskRunning || busy !== null}
                className="shrink-0 rounded-lg bg-sky-600 px-2.5 py-1 text-[10.5px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
              >
                下载环境
              </button>
            </div>
          )}
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
