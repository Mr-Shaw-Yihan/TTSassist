// 插件管理页：按功能分类组织 —— 语音合成（TTS 引擎）与语音输入（ASR 引擎）。
// 每个分类下：已安装引擎（完整卡片）+ 可安装/可更新条目（内置插件库与官方在线合并去重）。
// 安全：在线安装 zip SHA-256 对照官方索引；拖入安装 dll 对照 manifest.checksum，
// 来源可信度由用户确认弹窗把关。

import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  listPlugins,
  uninstallPlugin,
  installPluginZip,
  fetchPluginIndex,
  downloadInstallPlugin,
  listBundledPlugins,
  installBundledPlugin,
  importResourcePackFlow,
} from "../../services/invoke";
import { ResourcePackLinks } from "./ResourcePackLinks";
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

/** 可安装条目：内置插件库与在线索引合并去重后的统一形态 */
interface Candidate {
  id: string;
  name: string;
  version: string;
  description: string;
  requirements?: string | null;
  /** 安装来源：bundled=随安装包内置（离线即装） / online=官方在线下载 */
  source: "bundled" | "online";
}

/** 同 id 合并候选：优先内置（离线即装），同为内置/在线时保留更高版本 */
function mergeCandidates(
  bundled: BundledPluginInfo[],
  online: PluginIndexEntry[],
  type: string,
  typeOfOnline: (id: string) => string
): Candidate[] {
  const map = new Map<string, Candidate>();
  for (const b of bundled) {
    if ((b.plugin_type ?? "tts_engine") !== type) continue;
    map.set(b.id, {
      id: b.id,
      name: b.name,
      version: b.version,
      description: b.description,
      requirements: b.requirements,
      source: "bundled",
    });
  }
  for (const o of online) {
    if (typeOfOnline(o.id) !== type) continue;
    const prev = map.get(o.id);
    if (prev) {
      if (prev.source === "online" && isNewer(o.version, prev.version)) {
        map.set(o.id, {
          id: o.id,
          name: o.name,
          version: o.version,
          description: o.description,
          requirements: o.requirements,
          source: "online",
        });
      }
      // 已有内置候选：保留内置（离线即装），跳过在线
    } else {
      map.set(o.id, {
        id: o.id,
        name: o.name,
        version: o.version,
        description: o.description,
        requirements: o.requirements,
        source: "online",
      });
    }
  }
  return [...map.values()];
}

export function PluginPage() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 内置插件库（随安装包携带）
  const [bundled, setBundled] = useState<BundledPluginInfo[]>([]);

  // 在线插件索引（用户手动触发拉取，不自动联网）
  const [index, setIndex] = useState<PluginIndexEntry[] | null>(null);
  const [indexError, setIndexError] = useState<string | null>(null);
  const [indexLoading, setIndexLoading] = useState(false);

  // 安装/卸载等耗时操作（禁用按钮 + 提示）
  const [busy, setBusy] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  // 正在展示「安装方式二选一」面板的插件 id（在线下载 / 离线导入）
  const [envPick, setEnvPick] = useState<string | null>(null);

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
    void reloadBundled();
    // 在线索引不自动拉取：由页头「获取在线列表」按钮手动触发
  }, [reload, reloadBundled]);

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

  async function handleOpenLocation(p: PluginInfo) {
    try {
      await revealItemInDir(p.path);
    } catch (e) {
      window.alert(`打开插件目录失败：${e}`);
    }
  }

  /** 按来源安装候选条目（内置/在线两条安装通道统一入口） */
  async function handleInstallCandidate(c: Candidate) {
    setBusy(`正在安装「${c.name}」…`);
    try {
      const msg =
        c.source === "bundled"
          ? await installBundledPlugin(c.id)
          : await downloadInstallPlugin(c.id);
      await reload();
      await reloadBundled();
      window.alert(msg);
    } catch (e) {
      window.alert(`安装失败：${e}`);
    } finally {
      setBusy(null);
    }
  }

  /** 在线条目相对已装插件（用于判断可更新版本） */
  function onlineEntryOf(id: string): PluginIndexEntry | undefined {
    return index?.find((o) => o.id === id);
  }

  // ── 分类数据：已装插件按类型分组；可装条目内置+在线合并去重 ──────────
  // 老插件无 plugin_type 字段 → 默认归入语音合成（历史插件均为 TTS）
  const typeOf = (p: PluginInfo) => p.plugin_type ?? "tts_engine";
  const installedTts = plugins.filter((p) => typeOf(p) === "tts_engine");
  const installedAsr = plugins.filter((p) => typeOf(p) === "asr_engine");

  // 在线条目类型：新索引自带 plugin_type；旧索引无此字段时回退到同 id 的内置条目，再无则按 TTS
  const typeOfOnline = (id: string): string => {
    const entry = index?.find((o) => o.id === id);
    if (entry?.plugin_type) return entry.plugin_type;
    const b = bundled.find((x) => x.id === id);
    return b?.plugin_type ?? "tts_engine";
  };

  const candidatesTts = mergeCandidates(bundled, index ?? [], "tts_engine", typeOfOnline).filter(
    (c) => !plugins.some((p) => p.id === c.id)
  );
  const candidatesAsr = mergeCandidates(bundled, index ?? [], "asr_engine", typeOfOnline).filter(
    (c) => !plugins.some((p) => p.id === c.id)
  );

  // 索引获取结果提示用：可更新插件数 + 可新装条目数（避免「获取成功但无变化」的困惑）
  const updateCount = plugins.filter((p) => {
    const o = onlineEntryOf(p.id);
    return o && isNewer(o.version, p.version);
  }).length;
  const freshCount = candidatesTts.length + candidatesAsr.length;

  // ── 卡片与条目渲染 ─────────────────────────────────────────────

  /** 已安装引擎卡片（TTS/ASR 共用，按钮按类型差异化） */
  function PluginCard({ p }: { p: PluginInfo }) {
    const isAsr = typeOf(p) === "asr_engine";
    const isCurrentEngine = !isAsr && settings?.tts_engine === p.id;
    const isCurrentAsr =
      isAsr &&
      ((settings?.asr_plugin ? settings.asr_plugin === p.id : p.loaded) && p.loaded);
    const online = onlineEntryOf(p.id);
    const hasUpdate = online ? isNewer(online.version, p.version) : false;
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
        </div>

        {/* 操作行：引擎状态/更新在左，打开位置/卸载在右（独立一行，窄窗可换行） */}
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          {!isAsr &&
            (isCurrentEngine ? (
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
            ))}
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
          {hasUpdate && online && (
            <button
              onClick={() =>
                handleInstallCandidate({
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
          <button
            onClick={() => handleOpenLocation(p)}
            className="rounded-lg px-2 py-1 text-[11px] text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-600)]"
          >
            打开位置
          </button>
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

        {/* 本地引擎环境安装区：状态 / 下载按钮 / 进度面板 */}
        {p.loaded && p.has_setup && (
          <div className="mt-2">
            {task?.pluginId === p.id ? (
              <PluginSetupPanel pluginId={p.id} onClosed={() => void reload()} />
            ) : p.setup_status?.ready ? (
              <div className="flex items-center gap-1.5 rounded-lg border border-emerald-600/25 bg-emerald-600/5 px-3 py-2 text-[11px] text-emerald-700">
                <span>✓ 环境就绪 · 可离线使用</span>
                <span className="text-[var(--ink-300)]">
                  （已装音色 {p.setup_status.voices.length} 个）
                </span>
              </div>
            ) : envPick === p.id ? (
              // 安装方式二选一：在线下载（需代理） / 导入离线资源包
              <div className="rounded-lg border border-sky-600/25 bg-sky-600/5 px-3 py-2.5">
                <div className="mb-2 text-[11px] font-medium text-sky-800">
                  选择运行环境安装方式
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <button
                    onClick={() => {
                      setEnvPick(null);
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
                      从 HuggingFace 下载约 1.1GB，国内网络请先开启代理
                    </span>
                  </button>
                  <button
                    onClick={async () => {
                      setEnvPick(null);
                      try {
                        const done = await importResourcePackFlow(p.id);
                        if (done) void reload();
                      } catch (e) {
                        window.alert(`导入资源包失败：${e}`);
                      }
                    }}
                    disabled={taskRunning || busy !== null}
                    className="rounded-md border border-[var(--amber-500)]/60 px-2.5 py-2 text-left transition-colors hover:bg-[var(--amber-500)]/10 disabled:opacity-40"
                  >
                    <span className="block text-[11px] font-medium text-[var(--amber-600)]">
                      导入离线资源包（无需联网）
                    </span>
                    <span className="mt-0.5 block text-[10px] leading-relaxed text-[var(--ink-300)]">
                      下载 genie-resources-v1.zip（约 800MB）后选择导入，无需联网
                    </span>
                  </button>
                </div>
                <div className="mt-2 flex items-center justify-between gap-2">
                  <span className="text-[10px] leading-relaxed text-[var(--ink-300)]">
                    资源包获取：<ResourcePackLinks />
                  </span>
                  <button
                    onClick={() => setEnvPick(null)}
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
                  onClick={() => setEnvPick(p.id)}
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

  /** 可安装条目（内置/在线合并后的统一行） */
  function CandidateRow({ c }: { c: Candidate }) {
    return (
      <div className="flex items-center gap-3 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium text-[var(--ink-900)]">{c.name}</span>
            <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ink-500)]">
              v{c.version}
            </span>
            <span
              className="rounded-md bg-[var(--ink-100)]/60 px-1.5 py-0.5 text-[10px] text-[var(--ink-300)]"
              title={c.source === "bundled" ? "随安装包携带，无需联网即可安装" : "从官方渠道在线下载安装"}
            >
              {c.source === "bundled" ? "内置安装包" : "在线"}
            </span>
          </div>
          {c.description && (
            <p
              className="mt-1 truncate text-[11px] text-[var(--ink-500)]"
              title={c.description}
            >
              {c.description}
            </p>
          )}
          {c.requirements && (
            <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-300)]">
              资源需求：{c.requirements}
            </p>
          )}
        </div>
        <button
          onClick={() => handleInstallCandidate(c)}
          disabled={busy !== null}
          className="shrink-0 rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          安装
        </button>
      </div>
    );
  }

  /** 分类区块：标题 + 说明 + 已装卡片 + 可装条目 */
  function CategorySection({
    title,
    subtitle,
    installed,
    candidates,
    emptyHint,
  }: {
    title: string;
    subtitle: string;
    installed: PluginInfo[];
    candidates: Candidate[];
    emptyHint: string;
  }) {
    return (
      <section>
        {/* 分类头部：固定两行结构（标题行 + 描述行），两个分类格式统一 */}
        <div className="mb-3">
          <div className="flex items-center gap-2">
            <span className="h-3.5 w-[3px] shrink-0 rounded-full bg-[var(--amber-500)]" aria-hidden />
            <h2 className="font-display text-sm font-semibold tracking-wide text-[var(--ink-900)]">
              {title}
            </h2>
            <span className="rounded-md bg-[var(--ink-100)] px-1.5 py-0.5 text-[10px] text-[var(--ink-500)]">
              已装 {installed.length}
            </span>
          </div>
          <p className="mt-1 pl-[11px] text-[11px] leading-relaxed text-[var(--ink-300)]">
            {subtitle}
          </p>
        </div>

        {installed.length === 0 && candidates.length === 0 && !loading && !error && (
          <div className="rounded-xl border border-dashed border-[var(--ink-200)] px-4 py-6 text-center text-xs text-[var(--ink-300)]">
            {emptyHint}
          </div>
        )}

        <div className="space-y-3">
          {installed.map((p) => (
            <PluginCard key={p.id} p={p} />
          ))}
          {candidates.map((c) => (
            <CandidateRow key={`${c.source}-${c.id}`} c={c} />
          ))}
        </div>
      </section>
    );
  }

  return (
    <div className="relative flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-6 overflow-y-auto px-4 py-5">
        {/* 页头：说明在上，「获取在线列表」手动触发按钮在下 */}
        <div className="space-y-2">
          <p className="text-xs leading-relaxed text-[var(--ink-300)]">
            插件按用途分为「语音合成」与「语音输入」两类引擎。支持在线安装与拖入 zip 安装，
            安装前均做 SHA-256 完整性校验。
          </p>
          <button
            onClick={() => void reloadIndex()}
            disabled={indexLoading}
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
              className={indexLoading ? "animate-spin" : undefined}
              aria-hidden
            >
              <path d="M23 4v6h-6" />
              <path d="M1 20v-6h6" />
              <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
            </svg>
            {indexLoading ? "正在获取…" : index ? "刷新在线列表" : "获取在线列表"}
          </button>
          {/* 获取结果提示：成功但无可安装/可更新项时也要有明确反馈 */}
          {index && !indexLoading && !indexError && (
            <p className="text-[11px] text-[var(--ink-300)]">
              ✓ 在线列表已获取（{index.length} 个插件）：
              {updateCount + freshCount > 0
                ? `发现 ${updateCount + freshCount} 项可安装/更新，见下方分类`
                : "已装插件均为最新版本"}
            </p>
          )}
        </div>

        {loading && <div className="py-4 text-center text-sm text-[var(--ink-300)]">加载中…</div>}

        {error && (
          <div className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/5 px-3 py-2 text-xs text-[var(--seal)]">
            读取插件列表失败：{error}
          </div>
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

        {!loading && (
          <>
            {/* ── 语音合成（TTS 引擎） ── */}
            <CategorySection
              title="语音合成"
              subtitle="文字转语音的朗读引擎，可在设置-语音合成中切换"
              installed={installedTts}
              candidates={candidatesTts}
              emptyHint="尚未安装语音合成引擎插件，可从下方条目安装，或将插件 zip 拖入本窗口"
            />

            {/* ── 语音输入（ASR 引擎） ── */}
            <CategorySection
              title="语音输入"
              subtitle="说话转文字的识别引擎，快捷键与设备在设置-语音输入中配置"
              installed={installedAsr}
              candidates={candidatesAsr}
              emptyHint="尚未安装语音输入引擎插件，安装后即可用快捷键说话转文字"
            />
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
    </div>
  );
}
