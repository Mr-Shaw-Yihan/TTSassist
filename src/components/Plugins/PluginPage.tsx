// 插件管理页：已安装插件列表（状态/音色/卸载/设为引擎）。
// 在线插件市场与拖入安装在第 4 步加入。

import { useCallback, useEffect, useState } from "react";
import { listPlugins, uninstallPlugin } from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import type { PluginInfo } from "../../types";

export function PluginPage() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setPlugins(await listPlugins());
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function handleSetEngine(p: PluginInfo) {
    try {
      await patch("tts_engine", p.id);
    } catch (e) {
      window.alert(`切换引擎失败：${e}`);
    }
  }

  async function handleUninstall(p: PluginInfo) {
    const ok = window.confirm(
      `确定卸载插件「${p.name}」吗？\n\n` +
        (p.loaded
          ? "该插件正在使用中，卸载后本次会话内仍可用，重启应用后彻底移除。"
          : "卸载后立即生效。")
    );
    if (!ok) return;
    try {
      const msg = await uninstallPlugin(p.id);
      await reload();
      window.alert(msg);
    } catch (e) {
      window.alert(`卸载失败：${e}`);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-3 overflow-y-auto px-4 py-5">
        {/* 页头说明 */}
        <div className="text-xs leading-relaxed text-[var(--ink-300)]">
          插件为语音合成引擎提供扩展能力（如免费的 Edge TTS）。
          插件以动态库形式加载，安装前会做 SHA-256 完整性校验。
        </div>

        {loading && (
          <div className="mt-10 text-center text-sm text-[var(--ink-300)] animate-fade">加载中…</div>
        )}

        {error && (
          <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/5 px-3 py-2 text-xs text-[var(--seal)]">
            读取插件列表失败：{error}
          </div>
        )}

        {!loading && !error && plugins.length === 0 && (
          <div className="mt-14 flex flex-col items-center gap-2 text-[var(--ink-300)] animate-fade">
            <span className="font-display text-3xl text-[var(--ink-200)]">·</span>
            <p className="text-sm">尚未安装插件</p>
            <p className="text-xs">在线插件市场即将开放，也可通过插件 zip 包安装</p>
          </div>
        )}

        {plugins.map((p) => {
          const isCurrentEngine = settings?.tts_engine === p.id;
          return (
            <div
              key={p.id}
              className="animate-rise rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3.5 shadow-[0_1px_2px_rgba(26,24,22,0.03)]"
            >
              {/* 标题行：名称 + 版本 + 状态 */}
              <div className="flex items-center gap-2">
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
                <div className="flex-1" />
                {isCurrentEngine ? (
                  <span className="rounded-lg bg-[var(--amber-200)]/50 px-2 py-1 text-[11px] font-medium text-[var(--amber-600)]">
                    当前引擎
                  </span>
                ) : (
                  p.loaded && (
                    <button
                      onClick={() => handleSetEngine(p)}
                      className="rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
                    >
                      设为当前引擎
                    </button>
                  )
                )}
                <button
                  onClick={() => handleUninstall(p)}
                  className="rounded-lg px-2 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--seal)]/10 hover:text-[var(--seal)]"
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

              {/* 描述 */}
              {p.description && (
                <p className="mt-2 text-xs leading-relaxed text-[var(--ink-500)]">{p.description}</p>
              )}

              {/* 音色清单 */}
              {p.loaded && p.voices.length > 0 && (
                <div className="mt-2.5 flex flex-wrap gap-1.5">
                  {p.voices.map((v) => (
                    <span
                      key={v.id}
                      className="rounded-md border border-[var(--ink-200)] bg-[var(--paper)] px-2 py-0.5 text-[10px] text-[var(--ink-500)]"
                      title={v.id}
                    >
                      {v.label}
                    </span>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
