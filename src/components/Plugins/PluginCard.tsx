// 已安装插件卡片（模块级组件——此前定义在 PluginPage 函数体内，每次渲染都是新组件类型，
// 导致整页卡片 remount、animate-rise 重放闪烁；提到模块级后本地 state 与 DOM 得以保留）。
// 徽标与操作按插件类型差异化：TTS 引擎可设为当前引擎，ASR/服务/效果器仅展示与卸载。

import { useState } from "react";
import { PluginSetupPanel } from "./PluginSetupPanel";
import { ResourcePackLinks } from "./ResourcePackLinks";
import { PluginConfigPanel } from "../Settings/PluginConfigPanel";
import { useSettingsStore } from "../../stores/settingsStore";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import { importResourcePackFlow } from "../../services/invoke";
import { isNewer } from "../../utils/version";
import { toast } from "../common/Toast";
import type { PluginInfo, PluginIndexEntry } from "../../types";
import type { PluginActions } from "../../hooks/usePluginActions";

export function PluginCard({
  p,
  online,
  actions,
  busy,
  envPickId,
  setEnvPickId,
  onRefresh,
}: {
  p: PluginInfo;
  /** 在线索引中的同 id 条目（有新版本时显示更新按钮） */
  online?: PluginIndexEntry;
  actions: PluginActions;
  busy: string | null;
  /** 正在展示「安装方式二选一」面板的插件 id */
  envPickId: string | null;
  setEnvPickId: (id: string | null) => void;
  /** 安装/环境任务结束后刷新清单 */
  onRefresh: () => void;
}) {
  const isAsr = (p.plugin_type ?? "tts_engine") === "asr_engine";
  const isService = p.plugin_type === "service";
  const settings = useSettingsStore((s) => s.settings);
  const task = usePluginTaskStore((s) => s.task);
  const startEnv = usePluginTaskStore((s) => s.startEnv);
  const taskRunning = task?.status === "running";
  const isCurrentEngine = !isAsr && !isService && settings?.tts_engine === p.id;
  const isCurrentAsr =
    isAsr && ((settings?.asr_plugin ? settings.asr_plugin === p.id : p.loaded) && p.loaded);
  // 配置卡展开（卡片本地状态——remount 修复后才可靠）
  const [showConfig, setShowConfig] = useState(false);
  const hasConfig = (p.config?.fields.length ?? 0) > 0;
  // 有未填的必填字段时提示（不阻断安装/使用，合成时插件会报缺配置）
  const missingRequired = (p.config?.fields ?? []).filter(
    (f) => f.required && !(settings?.plugin_config?.[p.id]?.[f.key] ?? "").trim(),
  );

  return (
    <div className="animate-rise rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3.5 shadow-[0_1px_2px_rgba(26,24,22,0.03)]">
      {/* 标题行：名称 + 版本 + 状态徽标（允许换行，窄窗不溢出） */}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="text-sm font-medium text-[var(--ink-900)]">{p.name}</span>
        <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]">
          v{p.version}
        </span>
        {p.loaded ? (
          <span className="rounded-md bg-emerald-600/10 px-1.5 py-0.5 text-[10px] font-medium text-emerald-700">
            ✓ 已加载
          </span>
        ) : (
          <span
            className="rounded-md bg-[var(--seal)]/10 px-1.5 py-0.5 text-[10px] font-medium text-[var(--seal)]"
            title={p.error ?? undefined}
          >
            ✕ 加载失败
          </span>
        )}
        {p.category === "local" && (
          <span
            className="rounded-md bg-sky-600/10 px-1.5 py-0.5 text-[10px] font-medium text-sky-700"
            title="本地引擎：处理在本机完成，不依赖云端 API"
          >
            本地·离线
          </span>
        )}
        {missingRequired.length > 0 && (
          <button
            onClick={() => setShowConfig(true)}
            className="rounded-md bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-600"
            title={`待配置：${missingRequired.map((f) => f.label).join("、")}。点击在本卡内展开配置`}
          >
            ⚙ 待配置
          </button>
        )}
      </div>

      {/* 操作行：引擎状态/更新在左，配置/打开位置/卸载在右（独立一行，窄窗可换行） */}
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        {!isAsr && !isService &&
          (isCurrentEngine ? (
            <span className="rounded-lg bg-[var(--amber-200)]/50 px-2 py-1 text-[11px] font-medium text-[var(--amber-600)]">
              当前引擎
            </span>
          ) : (
            p.loaded && (
              <button
                onClick={() => void actions.setEngine(p)}
                className="rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
              >
                设为当前引擎
              </button>
            )
          ))}
        {isService && (
          <span className="px-2 py-1 text-[11px] text-[var(--ink-300)]">
            后台服务，配置在设置-插件服务中
          </span>
        )}
        {isAsr &&
          (isCurrentAsr ? (
            <span
              className="rounded-lg bg-violet-600/10 px-2 py-1 text-[11px] font-medium text-violet-700"
              title="语音输入当前使用的识别引擎，可在设置-语音输入中调整"
            >
              当前引擎
            </span>
          ) : (
            p.loaded && (
              <span className="px-2 py-1 text-[11px] text-[var(--ink-300)]">
                在设置-语音输入中切换
              </span>
            )
          ))}
        {online && isNewer(online.version, p.version) && (
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
            className="rounded-lg border border-[var(--amber-500)] px-2 py-1 text-[11px] font-medium text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-200)]/30 disabled:opacity-40"
            title={`在线有新版本 v${online.version}，点击更新`}
          >
            更新至 v{online.version}
          </button>
        )}
        <div className="flex-1" />
        {hasConfig && (
          <button
            onClick={() => setShowConfig((v) => !v)}
            disabled={busy !== null}
            className="rounded-lg px-2 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-600)] disabled:opacity-40"
          >
            {showConfig ? "收起配置" : "配置"}
          </button>
        )}
        <button
          onClick={() => void actions.openLocation(p)}
          className="rounded-lg px-2 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-600)]"
        >
          打开位置
        </button>
        <button
          onClick={() => void actions.uninstall(p)}
          disabled={busy !== null}
          className="rounded-lg px-2 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--seal)]/10 hover:text-[var(--seal)] disabled:opacity-40"
        >
          卸载
        </button>
      </div>

      {/* 失败原因 */}
      {!p.loaded && p.error && (
        <div className="mt-2 rounded-lg border border-[var(--seal)]/20 bg-[var(--seal)]/5 px-3 py-2 text-[11px] leading-relaxed text-[var(--seal)]">
          {p.error}
        </div>
      )}

      {/* 描述（窄窗口下可能显示不全，悬停查看完整内容） */}
      {p.description && (
        <p
          className="mt-2 text-xs leading-relaxed text-[var(--ink-500)]"
          title={p.description}
        >
          {p.description}
        </p>
      )}

      {/* 资源需求（供用户下载运行环境前判断配置） */}
      {p.requirements && (
        <div className="mt-2 rounded-lg border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/40 px-2.5 py-1.5 text-[11px] leading-relaxed text-[var(--ink-500)]">
          <span className="font-medium text-[var(--ink-700)]">资源需求：</span>
          {p.requirements}
        </div>
      )}

      {/* 通用插件配置卡：本卡内直接展开，免去「去设置页选中引擎」的深层入口 */}
      {hasConfig && showConfig && (
        <div className="mt-2">
          <PluginConfigPanel key={p.id} pluginId={p.id} pluginName={p.name} />
        </div>
      )}

      {/* 本地引擎环境安装区：状态 / 下载按钮 / 进度面板 */}
      {p.loaded && p.has_setup && (
        <div className="mt-2">
          {task?.pluginId === p.id ? (
            <PluginSetupPanel pluginId={p.id} onClosed={onRefresh} />
          ) : p.setup_status?.ready ? (
            <div className="flex items-center gap-1.5 rounded-lg border border-emerald-600/25 bg-emerald-600/5 px-3 py-2 text-[11px] text-emerald-700">
              <span>✓ 环境就绪 · 可离线使用</span>
              <span className="text-[var(--ink-300)]">
                （已装音色 {p.setup_status.voices.length} 个）
              </span>
            </div>
          ) : envPickId === p.id ? (
            // 安装方式二选一：在线下载（需代理） / 导入离线资源包
            <div className="rounded-lg border border-sky-600/25 bg-sky-600/5 px-3 py-2.5">
              <div className="mb-2 text-[11px] font-medium text-sky-800">
                选择运行环境安装方式
              </div>
              <div className="grid grid-cols-2 gap-2">
                <button
                  onClick={() => {
                    setEnvPickId(null);
                    startEnv(p.id, p.name).catch(() => {
                      /* 错误已在 store */
                    });
                  }}
                  disabled={taskRunning || busy !== null}
                  className="rounded-md border border-sky-600/40 px-2.5 py-2 text-left transition-colors hover:bg-sky-600/10 disabled:opacity-40"
                >
                  <span className="block text-[11px] font-medium text-sky-700">
                    在线下载（需魔法上网）
                  </span>
                  <span className="mt-0.5 block text-[10px] leading-relaxed text-[var(--ink-300)]">
                    从 HuggingFace 下载运行环境与模型，国内网络请先开启代理
                  </span>
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
                  className="rounded-md border border-[var(--amber-500)]/60 px-2.5 py-2 text-left transition-colors hover:bg-[var(--amber-500)]/10 disabled:opacity-40"
                >
                  <span className="block text-[11px] font-medium text-[var(--amber-600)]">
                    导入离线资源包（无需联网）
                  </span>
                  <span className="mt-0.5 block text-[10px] leading-relaxed text-[var(--ink-300)]">
                    获取该插件的离线资源包后选择导入，全程无需联网
                  </span>
                </button>
              </div>
              <div className="mt-2 flex items-center justify-between gap-2">
                <span className="text-[10px] leading-relaxed text-[var(--ink-300)]">
                  {/* 资源包网盘/QQ群渠道为 genie 专属资产；其他插件提示向提供方获取 */}
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
                  className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
                >
                  取消
                </button>
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-2 rounded-lg border border-sky-600/25 bg-sky-600/5 px-3 py-2">
              <span
                className="min-w-0 flex-1 truncate text-[11px] text-sky-800"
                title={p.setup_status?.summary ?? ""}
              >
                {p.setup_status?.summary ?? "运行环境未安装"}
              </span>
              <button
                onClick={() => setEnvPickId(p.id)}
                disabled={taskRunning || busy !== null}
                className="shrink-0 rounded-lg bg-sky-600 px-3 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
              >
                下载运行环境
              </button>
            </div>
          )}
        </div>
      )}

      {/* 音色清单（仅 TTS 引擎有意义；最多展示 6 个，避免云端引擎音色过多撞爆版面） */}
      {!isAsr && p.loaded && p.voices.length > 0 && (
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {p.voices.slice(0, 6).map((v) => (
            <span
              key={v.id}
              className="rounded-md border border-[var(--ink-200)] bg-[var(--paper)] px-2 py-0.5 text-[10px] text-[var(--ink-500)]"
              title={v.id}
            >
              {v.label}
            </span>
          ))}
          {p.voices.length > 6 && (
            <span
              className="rounded-md border border-dashed border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--ink-300)]"
              title={p.voices.slice(6).map((v) => v.label).join("、")}
            >
              +{p.voices.length - 6} 更多
            </span>
          )}
        </div>
      )}
    </div>
  );
}
