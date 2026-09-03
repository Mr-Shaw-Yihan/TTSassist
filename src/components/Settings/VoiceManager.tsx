// 音色管理面板（阶段 21）：本地引擎音色的安装 / 卸载 / 导入自定义音色包。
//
// 渲染在设置页对应插件的音色下拉框下方（仅 has_voice_management 的插件显示）。
// 安装任务经全局 usePluginTaskStore 发起（进度面板在音色下拉框下方统一展示），
// 本组件按任务状态禁用按钮，避免并发安装。
//
// 规则：
// - 全局有安装任务时，安装/卸载/导入按钮全部禁用；
// - 使用中的音色禁止卸载（先切换到其他音色）；
// - 卸载需二次确认；导入为"复制"，保留用户原文件夹。

import { useState, type ReactNode } from "react";
import {
  uninstallVoice,
  importVoicePack,
  pickVoicePackFolder,
} from "../../services/invoke";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import { SectionHeading } from "../common/SettingsSection";
import type { PluginInfo } from "../../types";

interface Props {
  plugin: PluginInfo;
  /** 该插件当前选中的音色 id（用于禁止卸载在用音色） */
  currentVoiceId: string;
  /** 音色表/状态变化后的刷新回调（父组件重查 list_plugins） */
  onChanged: () => void;
  /** 「当前音色」下拉插槽（并入本卡顶部，与其它引擎音色管理卡同构） */
  voiceSelect?: ReactNode;
}

export function VoiceManager({ plugin, currentVoiceId, onChanged, voiceSelect }: Props) {
  const task = usePluginTaskStore((s) => s.task);
  const startVoice = usePluginTaskStore((s) => s.startVoice);
  const taskRunning = task?.status === "running";

  const installed = plugin.setup_status?.voices ?? [];
  // 行内卸载二次确认（记录待确认的音色 id）
  const [confirmUninstall, setConfirmUninstall] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  // 行内反馈（卸载/导入的结果或错误，短暂展示）
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null);

  function showNotice(text: string, error = false) {
    setNotice({ text, error });
    setTimeout(() => setNotice(null), 4000);
  }

  function displayName(voiceId: string): string {
    const v = plugin.voices.find((x) => x.id === voiceId);
    // 去掉未安装后缀"· 待下载"，得到干净展示名
    return (v?.label ?? voiceId).replace(/\s*·\s*待下载$/, "");
  }

  function handleInstall(voiceId: string) {
    startVoice(plugin.id, voiceId, displayName(voiceId)).then(
      () => onChanged(),
      () => {
        /* 错误已记录在 store，进度面板展示 */
      },
    );
  }

  async function handleUninstall(voiceId: string) {
    setConfirmUninstall(null);
    try {
      const msg = await uninstallVoice(plugin.id, voiceId);
      showNotice(msg);
      onChanged();
    } catch (e) {
      showNotice(String(e), true);
    }
  }

  async function handleImport() {
    const dir = await pickVoicePackFolder();
    if (!dir) return;
    setImporting(true);
    try {
      const msg = await importVoicePack(plugin.id, dir);
      showNotice(msg);
      onChanged();
    } catch (e) {
      showNotice(String(e), true);
    } finally {
      setImporting(false);
    }
  }

  return (
    <div className="rounded-xl border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/25 px-3.5 py-3">
      <SectionHeading
        title="音色管理"
        desc="选择当前音色，或安装 / 卸载 / 导入本地音色包。"
        right={
          <span className="text-[10px] text-[var(--ink-300)]">已安装 {installed.length} 个</span>
        }
      />

      {voiceSelect && <div className="mt-2.5">{voiceSelect}</div>}

      <div className="mt-2.5 space-y-1.5">
        {plugin.voices.map((v) => {
          const isInstalled = installed.includes(v.id);
          const isCurrent = v.id === currentVoiceId;
          const isTaskTarget = taskRunning && task?.kind === "voice" && task.voiceId === v.id;
          return (
            <div
              key={v.id}
              className="flex items-center gap-2 rounded-lg border border-[var(--ink-200)]/70 px-2.5 py-1.5"
            >
              <span className="min-w-0 flex-1 truncate text-[11px] text-[var(--ink-700)]" title={displayName(v.id)}>
                {displayName(v.id)}
                {isCurrent && (
                  <span className="ml-1.5 rounded bg-[var(--amber-200)]/50 px-1 py-0.5 text-[9px] font-medium text-[var(--amber-600)]">
                    使用中
                  </span>
                )}
              </span>

              {isInstalled ? (
                confirmUninstall === v.id ? (
                  <>
                    <span className="text-[10px] text-[var(--seal)]">删除后可重新下载</span>
                    <button
                      onClick={() => void handleUninstall(v.id)}
                      className="shrink-0 rounded-md bg-[var(--seal)] px-2 py-0.5 text-[10px] font-medium text-white hover:opacity-90"
                    >
                      确认卸载
                    </button>
                    <button
                      onClick={() => setConfirmUninstall(null)}
                      className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
                    >
                      取消
                    </button>
                  </>
                ) : (
                  <button
                    onClick={() => setConfirmUninstall(v.id)}
                    disabled={isCurrent || taskRunning}
                    title={isCurrent ? "请先切换到其他音色再卸载" : undefined}
                    className="shrink-0 rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--ink-500)] transition-colors hover:border-[var(--seal)] hover:text-[var(--seal)] disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    卸载
                  </button>
                )
              ) : (
                <button
                  onClick={() => handleInstall(v.id)}
                  disabled={taskRunning}
                  className="shrink-0 rounded-md bg-sky-600 px-2 py-0.5 text-[10px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
                >
                  {isTaskTarget ? "下载中…" : "安装"}
                </button>
              )}
            </div>
          );
        })}
      </div>

      {/* 导入自定义音色包 */}
      <button
        onClick={() => void handleImport()}
        disabled={importing || taskRunning}
        className="mt-2 w-full rounded-lg border border-dashed border-[var(--ink-200)] px-3 py-1.5 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-40"
      >
        {importing ? "导入中…" : "+ 导入自定义音色包（选择文件夹）"}
      </button>
      <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
        音色包需包含 tts_models/ 与 prompt_wav.json；导入为复制，保留原文件夹。
      </p>

      {notice && (
        <div
          className={[
            "mt-2 rounded-lg px-2.5 py-1.5 text-[11px] leading-relaxed",
            notice.error
              ? "bg-[var(--seal)]/10 text-[var(--seal)]"
              : "bg-emerald-600/5 text-emerald-700",
          ].join(" ")}
        >
          {notice.text}
        </div>
      )}
    </div>
  );
}
