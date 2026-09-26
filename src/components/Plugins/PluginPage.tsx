// 插件管理页（编排层）：数据在三源 hook（usePluginInventory），动作用统一命令层（usePluginActions），
// 卡片/条目/分类在 PluginCard / PluginSections。本文件只做装配：页头、分类、拖入安装、全局提示。
// 安全：在线安装 zip SHA-256 对照官方索引；拖入安装 dll 对照 manifest.checksum，
// 来源可信度由确认弹窗把关。

import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { usePluginInventory } from "../../hooks/usePluginInventory";
import { usePluginActions } from "../../hooks/usePluginActions";
import { CategorySection } from "./PluginSections";
import { ConfirmDialogHost } from "../common/ConfirmDialog";
import { ToastHost } from "../common/Toast";

export function PluginPage() {
  const inv = usePluginInventory();
  const actions = usePluginActions(inv.reload, inv.reloadBundled);
  const { busy } = actions;

  // 正在展示「安装方式二选一」面板的插件 id（在线下载 / 离线导入）
  const [envPickId, setEnvPickId] = useState<string | null>(null);
  // 拖入 zip 时的浮层开关
  const [dragOver, setDragOver] = useState(false);

  // 拖入安装：监听窗口拖放事件（dropInstall 为稳定引用，不随数据刷新重订阅）
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    (async () => {
      const u = await getCurrentWindow().onDragDropEvent(async (event) => {
        const payload = event.payload;
        if (payload.type === "enter") {
          if (payload.paths.some((p) => p.toLowerCase().endsWith(".zip"))) setDragOver(true);
        } else if (payload.type === "leave") {
          setDragOver(false);
        } else if (payload.type === "drop") {
          setDragOver(false);
          const zip = payload.paths.find((p) => p.toLowerCase().endsWith(".zip"));
          if (zip) await actions.dropInstall(zip);
        }
      });
      if (cancelled) {
        u();
      } else {
        unlisten = u;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [actions.dropInstall]);

  return (
    <div className="relative flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-6 overflow-y-auto px-4 py-5">
        {/* 页头：说明在上，「获取在线列表」手动触发按钮在下 */}
        <div className="space-y-2">
          <p className="text-xs leading-relaxed text-[var(--ink-300)]">
            插件按用途分为「语音合成」「语音输入」「语音效果器」等类别。支持在线安装与拖入 zip 安装，
            安装前均做 SHA-256 完整性校验。
          </p>
          <button
            onClick={() => void inv.reloadIndex()}
            disabled={inv.indexLoading}
            title="联网获取官方在线插件列表（含可更新版本）"
            className="inline-flex items-center gap-1.5 rounded-lg border border-[var(--ink-200)] px-2.5 py-1.5 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:cursor-wait disabled:opacity-60"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              className={inv.indexLoading ? "animate-spin" : undefined}
              aria-hidden
            >
              <path d="M23 4v6h-6" />
              <path d="M1 20v-6h6" />
              <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
            </svg>
            {inv.indexLoading ? "正在获取…" : inv.index ? "刷新在线列表" : "获取在线列表"}
          </button>
          {/* 获取结果提示：成功但无可安装/可更新项时也要有明确反馈 */}
          {inv.index && !inv.indexLoading && !inv.indexError && (
            <p className="text-[11px] text-[var(--ink-300)]">
              ✓ 在线列表已获取（{inv.index.length} 个插件）：
              {inv.updateCount + inv.freshCount > 0
                ? `发现 ${inv.updateCount + inv.freshCount} 项可安装/更新，见下方分类`
                : "已装插件均为最新版本"}
            </p>
          )}
        </div>

        {inv.loading && (
          <div className="py-4 text-center text-sm text-[var(--ink-300)]">加载中…</div>
        )}

        {inv.error && (
          <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/5 px-3 py-2 text-xs text-[var(--seal)]">
            读取插件列表失败：{inv.error}
          </div>
        )}

        {inv.indexError && (
          <div className="rounded-lg border border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2.5 text-xs leading-relaxed text-[var(--ink-500)]">
            无法获取在线插件列表：{inv.indexError}
            <button
              onClick={() => void inv.reloadIndex()}
              className="ml-2 text-[var(--amber-600)] underline underline-offset-2"
            >
              重试
            </button>
          </div>
        )}

        {!inv.loading && (
          <>
            {/* ── 语音合成（TTS 引擎） ── */}
            <CategorySection
              inv={inv}
              title="语音合成"
              subtitle="文字转语音的朗读引擎，可在设置-语音合成中切换"
              installed={inv.installedTts}
              candidates={inv.candidatesTts}
              emptyHint="尚未安装语音合成引擎插件，可从下方条目安装，或将插件 zip 拖入本窗口"
              actions={actions}
              busy={busy}
              envPickId={envPickId}
              setEnvPickId={setEnvPickId}
            />

            {/* ── 语音输入（ASR 引擎） ── */}
            <CategorySection
              inv={inv}
              title="语音输入"
              subtitle="说话转文字的识别引擎，快捷键与设备在设置-语音输入中配置"
              installed={inv.installedAsr}
              candidates={inv.candidatesAsr}
              emptyHint="尚未安装语音输入引擎插件，安装后即可用快捷键说话转文字"
              actions={actions}
              busy={busy}
              envPickId={envPickId}
              setEnvPickId={setEnvPickId}
            />

            {/* ── 语音效果器（audio_effect） ── */}
            <CategorySection
              inv={inv}
              title="语音效果器"
              subtitle="给合成语音套预设声线；选择与排序在设置-语音合成的效果器分区"
              installed={inv.installedFx}
              candidates={inv.candidatesFx}
              emptyHint="尚未安装效果器插件，内置效果器包会在安装包内自动出现"
              actions={actions}
              busy={busy}
              envPickId={envPickId}
              setEnvPickId={setEnvPickId}
            />

            {/* ── 服务插件（type=service，仅已装展示） ── */}
            {inv.installedService.length > 0 && (
              <CategorySection
                inv={inv}
                title="服务插件"
                subtitle="不参与合成/识别的后台能力插件，配置见设置-插件服务"
                installed={inv.installedService}
                candidates={[]}
                emptyHint=""
                actions={actions}
                busy={busy}
                envPickId={envPickId}
                setEnvPickId={setEnvPickId}
              />
            )}
          </>
        )}
      </div>

      {/* 拖入提示浮层 */}
      {dragOver && (
        <div className="pointer-events-none absolute inset-2 z-10 flex items-center justify-center rounded-2xl border-2 border-dashed border-[var(--amber-500)] bg-[var(--amber-200)]/20">
          <span className="rounded-xl bg-[var(--paper-card)] px-4 py-2 text-sm font-medium text-[var(--amber-600)] shadow-sm">
            松开以安装插件 zip
          </span>
        </div>
      )}

      {/* 操作中提示条 */}
      {busy && (
        <div className="border-t border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-2 text-xs text-[var(--amber-600)] animate-fade">
          {busy}
        </div>
      )}

      <ToastHost />
      <ConfirmDialogHost />
    </div>
  );
}
