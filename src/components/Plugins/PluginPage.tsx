// 插件管理页：已装插件 + 在线插件（官方索引）+ 拖入安装。
// 安全：在线安装 zip SHA-256 对照官方索引；拖入安装 dll 对照 manifest.checksum，
// 来源可信度由用户确认弹窗把关。

import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  listPlugins,
  uninstallPlugin,
  installPluginZip,
  fetchPluginIndex,
  downloadInstallPlugin,
  listBundledPlugins,
  installBundledPlugin,
} from "../../services/invoke";
import { useSettingsStore } from "../../stores/settingsStore";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import { PluginSetupPanel } from "./PluginSetupPanel";
import type { PluginInfo, PluginIndexEntry, BundledPluginInfo } from "../../types";

/** 版本号比较：a > b 返回 true（按数字段逐段比） */
function isNewer(a: string, b: string): boolean {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x > y) return true;
    if (x < y) return false;
  }
  return false;
}

export function PluginPage() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 内置插件库（随安装包携带）
  const [bundled, setBundled] = useState<BundledPluginInfo[]>([]);

  // 在线插件索引
  const [index, setIndex] = useState<PluginIndexEntry[] | null>(null);
  const [indexError, setIndexError] = useState<string | null>(null);
  const [indexLoading, setIndexLoading] = useState(true);

  // 安装/卸载等耗时操作（禁用按钮 + 提示）
  const [busy, setBusy] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);

  // 环境安装任务走全局任务 store（启动在按钮点击处，面板只订阅）
  const task = usePluginTaskStore((s) => s.task);
  const startEnv = usePluginTaskStore((s) => s.startEnv);
  const taskRunning = task?.status === "running";

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

  const reloadIndex = useCallback(async () => {
    setIndexLoading(true);
    try {
      setIndex(await fetchPluginIndex());
      setIndexError(null);
    } catch (e) {
      setIndex(null);
      setIndexError(String(e));
    } finally {
      setIndexLoading(false);
    }
  }, []);

  const reloadBundled = useCallback(async () => {
    try {
      setBundled(await listBundledPlugins());
    } catch {
      setBundled([]);
    }
  }, []);

  useEffect(() => {
    void reload();
    void reloadIndex();
    void reloadBundled();
  }, [reload, reloadIndex, reloadBundled]);

  // 拖入安装：监听窗口拖放事件
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
          if (zip) await handleDropInstall(zip);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plugins, busy]);

  async function handleDropInstall(zipPath: string) {
    const ok = window.confirm(
      "检测到拖入的插件安装包。\n\n" +
        "提示：该插件来自本地文件，非官方索引渠道。系统会校验插件完整性（SHA-256），" +
        "但无法验证来源可信度，请确认你信任该插件的来源。\n\n" +
        `是否安装？\n${zipPath}`
    );
    if (!ok) return;
    setBusy("正在安装本地插件…");
    try {
      const msg = await installPluginZip(zipPath);
      await reload();
      window.alert(msg);
    } catch (e) {
      window.alert(`安装失败：${e}`);
    } finally {
      setBusy(null);
    }
  }

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
    setBusy("正在卸载…");
    try {
      const msg = await uninstallPlugin(p.id);
      await reload();
      window.alert(msg);
    } catch (e) {
      window.alert(`卸载失败：${e}`);
    } finally {
      setBusy(null);
    }
  }

  async function handleOnlineInstall(entry: PluginIndexEntry) {
    setBusy(`正在下载安装「${entry.name}」…`);
    try {
      const msg = await downloadInstallPlugin(entry.id);
      await reload();
      window.alert(msg);
    } catch (e) {
      window.alert(`安装失败：${e}`);
    } finally {
      setBusy(null);
    }
  }

  async function handleBundledInstall(entry: BundledPluginInfo) {
    setBusy(`正在安装「${entry.name}」…`);
    try {
      const msg = await installBundledPlugin(entry.id);
      await reload();
      await reloadBundled();
      window.alert(msg);
    } catch (e) {
      window.alert(`安装失败：${e}`);
    } finally {
      setBusy(null);
    }
  }

  /** 在线条目相对已装插件的状态 */
  function installedOf(entry: PluginIndexEntry): PluginInfo | undefined {
    return plugins.find((p) => p.id === entry.id);
  }

  return (
    <div className="relative flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-5 overflow-y-auto px-4 py-5">
        {/* 页头说明 */}
        <div className="text-xs leading-relaxed text-[var(--ink-300)]">
          插件为语音合成提供扩展引擎（如免费的 Edge TTS）。支持在线安装与拖入 zip 安装，
          安装前均做 SHA-256 完整性校验。
        </div>

        {/* ── 已安装 ── */}
        <section>
          <h2 className="mb-2 text-[11px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">
            已安装插件
          </h2>

          {loading && <div className="py-4 text-center text-sm text-[var(--ink-300)]">加载中…</div>}

          {error && (
            <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/5 px-3 py-2 text-xs text-[var(--seal)]">
              读取插件列表失败：{error}
            </div>
          )}

          {!loading && !error && plugins.length === 0 && (
            <div className="rounded-xl border border-dashed border-[var(--ink-200)] px-4 py-8 text-center text-xs text-[var(--ink-300)] animate-fade">
              尚未安装插件，可从下方在线列表安装，或将插件 zip 拖入本窗口
            </div>
          )}

          <div className="space-y-3">
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
                    {p.category === "local" && (
                      <span
                        className="rounded-md bg-sky-600/10 px-1.5 py-0.5 text-[10px] font-medium text-sky-700"
                        title="本地引擎：合成在本机完成，不依赖云端 API"
                      >
                        本地·离线
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

                  {/* 描述 */}
                  {p.description && (
                    <p className="mt-2 text-xs leading-relaxed text-[var(--ink-500)]">{p.description}</p>
                  )}

                  {/* 资源需求（供用户下载运行环境前判断配置） */}
                  {p.requirements && (
                    <div className="mt-2 rounded-lg border border-[var(--ink-200)]/70 bg-[var(--ink-100)]/40 px-2.5 py-1.5 text-[11px] leading-relaxed text-[var(--ink-500)]">
                      <span className="font-medium text-[var(--ink-700)]">资源需求：</span>
                      {p.requirements}
                    </div>
                  )}

                  {/* 本地引擎环境安装区：状态 / 下载按钮 / 进度面板 */}
                  {p.loaded && p.has_setup && (
                    <div className="mt-2">
                      {task?.pluginId === p.id ? (
                        <PluginSetupPanel
                          pluginId={p.id}
                          onClosed={() => void reload()}
                        />
                      ) : p.setup_status?.ready ? (
                        <div className="flex items-center gap-1.5 rounded-lg border border-emerald-600/25 bg-emerald-600/5 px-3 py-2 text-[11px] text-emerald-700">
                          <span>✓ 环境就绪 · 可离线使用</span>
                          <span className="text-[var(--ink-300)]">
                            （已装音色 {p.setup_status.voices.length} 个）
                          </span>
                        </div>
                      ) : (
                        <div className="flex items-center gap-2 rounded-lg border border-sky-600/25 bg-sky-600/5 px-3 py-2">
                          <span className="min-w-0 flex-1 truncate text-[11px] text-sky-800" title={p.setup_status?.summary ?? ""}>
                            {p.setup_status?.summary ?? "运行环境未安装"}
                          </span>
                          <button
                            onClick={() => {
                              startEnv(p.id, p.name).catch(() => { /* 错误已在 store */ });
                            }}
                            disabled={taskRunning || busy !== null}
                            className="shrink-0 rounded-lg bg-sky-600 px-3 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
                          >
                            下载运行环境
                          </button>
                        </div>
                      )}
                    </div>
                  )}

                  {/* 音色清单（最多展示 6 个，避免云端引擎音色过多撞爆版面） */}
                  {p.loaded && p.voices.length > 0 && (
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
            })}
          </div>
        </section>

        {/* ── 插件库（随安装包内置，离线可用） ── */}
        {bundled.length > 0 && (
          <section>
            <h2 className="mb-2 text-[11px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">
              插件库（随安装包提供）
            </h2>
            <div className="space-y-2.5">
              {bundled.map((entry) => (
                <div
                  key={entry.id}
                  className="flex items-center gap-3 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-[var(--ink-900)]">{entry.name}</span>
                      <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]">
                        v{entry.version}
                      </span>
                    </div>
                    {entry.description && (
                      <p className="mt-1 truncate text-[11px] text-[var(--ink-500)]">{entry.description}</p>
                    )}
                    {entry.requirements && (
                      <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
                        资源需求：{entry.requirements}
                      </p>
                    )}
                  </div>
                  {entry.installed ? (
                    <span className="shrink-0 text-[11px] text-[var(--ink-300)]">已安装</span>
                  ) : (
                    <button
                      onClick={() => handleBundledInstall(entry)}
                      disabled={busy !== null}
                      className="shrink-0 rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-40"
                    >
                      安装
                    </button>
                  )}
                </div>
              ))}
            </div>
          </section>
        )}

        {/* ── 在线插件（官方索引） ── */}
        <section>
          <h2 className="mb-2 text-[11px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">
            在线插件（官方渠道）
          </h2>

          {indexLoading && (
            <div className="py-4 text-center text-xs text-[var(--ink-300)]">正在获取插件索引…</div>
          )}

          {indexError && (
            <div className="rounded-lg border border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2.5 text-xs leading-relaxed text-[var(--ink-500)]">
              无法获取在线插件列表：{indexError}
              <button
                onClick={() => void reloadIndex()}
                className="ml-2 text-[var(--amber-600)] underline underline-offset-2"
              >
                重试
              </button>
            </div>
          )}

          {index && index.length === 0 && (
            <div className="rounded-xl border border-dashed border-[var(--ink-200)] px-4 py-6 text-center text-xs text-[var(--ink-300)]">
              官方索引暂无可安装插件
            </div>
          )}

          {index && index.length > 0 && (
            <div className="space-y-2.5">
              {index.map((entry) => {
                const installed = installedOf(entry);
                const hasUpdate = installed ? isNewer(entry.version, installed.version) : false;
                return (
                  <div
                    key={entry.id}
                    className="flex items-center gap-3 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-[var(--ink-900)]">{entry.name}</span>
                        <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]">
                          v{entry.version}
                        </span>
                      </div>
                      {entry.description && (
                        <p className="mt-1 truncate text-[11px] text-[var(--ink-500)]">{entry.description}</p>
                      )}
                      {entry.requirements && (
                        <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
                          资源需求：{entry.requirements}
                        </p>
                      )}
                    </div>
                    {!installed && (
                      <button
                        onClick={() => handleOnlineInstall(entry)}
                        disabled={busy !== null}
                        className="shrink-0 rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-40"
                      >
                        安装
                      </button>
                    )}
                    {installed && hasUpdate && (
                      <button
                        onClick={() => handleOnlineInstall(entry)}
                        disabled={busy !== null}
                        className="shrink-0 rounded-lg border border-[var(--amber-500)] px-3 py-1.5 text-[11px] font-medium text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-200)]/30 disabled:opacity-40"
                      >
                        更新
                      </button>
                    )}
                    {installed && !hasUpdate && (
                      <span className="shrink-0 text-[11px] text-[var(--ink-300)]">已安装</span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </section>
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
    </div>
  );
}
