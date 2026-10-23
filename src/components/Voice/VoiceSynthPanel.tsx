// 语音中心 · TTS 合成面板：默认引擎选择 + 逐引擎配置（引擎一律是插件，基座不认引擎身份）。
// 从设置页「语音合成」区整体析出，行为保持一致；MiniMax 国际克隆/管理交由 MinimaxVoicePanel 承担。
// 引擎以 EngineCard 同构呈现；插件自述需要装音色时走页内确认卡（确认后经全局任务 store 启动）。

import { useState, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import {
  listPlugins,
  preloadVoice,
  promptEngineWarmup,
} from "../../services/invoke";
import type { PluginInfo } from "../../types";
import { hasVoiceLibrary } from "../../utils/pluginCapability";
import { Field, SubPanel } from "../common/SettingsSection";
import { CustomVoicePanel } from "./CustomVoicePanel";
import { EngineCard } from "./EngineCard";
import { MinimaxVoicePanel } from "./MinimaxVoicePanel";
import { CloneSamplePanel } from "./CloneSamplePanel";
import { PluginSetupPanel } from "../Plugins/PluginSetupPanel";
import { PluginConfigPanel } from "../Settings/PluginConfigPanel";
import { VoiceManager } from "../Settings/VoiceManager";

export function VoiceSynthPanel() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);

  // 已加载插件（引擎下拉动态项 + 插件音色来源）
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  useEffect(() => {
    listPlugins().then(setPlugins).catch(() => {});
  }, []);

  // 安装任务走全局 store（启动在用户确认处，进度面板只订阅）
  const task = usePluginTaskStore((s) => s.task);
  const startVoice = usePluginTaskStore((s) => s.startVoice);
  const taskRunning = task?.status === "running";

  // 待确认项（页内确认卡片，替代 window.confirm，且确认前不改配置）
  const [pendingVoice, setPendingVoice] = useState<
    { pluginId: string; voiceId: string; label: string } | null
  >(null);
  // 正在预加载的音色（切换已装音色时的瞬时指示）
  const [preloadingVoice, setPreloadingVoice] = useState<string | null>(null);

  // 没拿到设置前用空串，不拿任何引擎名兜底：默认引擎是宿主的职责
  //（Settings::default 已给出），前端再兜一层只会多一处基座里的引擎身份字面量
  const engineId = settings?.tts_engine ?? "";

  /** 取插件音色的干净展示名（去掉"· 待下载"后缀） */
  function voiceLabel(plugin: PluginInfo, voiceId: string): string {
    const v = plugin.voices.find((x) => x.id === voiceId);
    return (v?.label ?? voiceId).replace(/\s*·\s*待下载$/, "");
  }

  /** 切换引擎：环境已就绪的本地引擎询问是否后台预热（避免首次对话长等待） */
  function handleEngineChange(newId: string) {
    void patch("tts_engine", newId);
    const p = plugins.find((x) => x.id === newId);
    if (p?.category === "local" && p.setup_status?.ready) {
      const vid = settings?.plugin_voices?.[p.id] ?? p.voices[0]?.id ?? "";
      if (vid) void promptEngineWarmup(p.name, p.id, vid);
    }
  }

  /** 切换插件音色：
   *  - 已安装 → 立即切换 + 后台预加载（有瞬时指示）；
   *  - 未安装 → 先弹页内确认卡片，确认并【安装成功后】才切换（确认前不动配置）。
   *  已经落定的切换会回传 patch 的 Promise，供调用方（如克隆样本卡）串行等它写完。 */
  function handlePluginVoiceChange(plugin: PluginInfo, voiceId: string): Promise<void> | undefined {
    // 无音色管理能力的插件（云端引擎）：音色即开即用，直接切换
    if (!plugin.has_voice_management) {
      return patch("plugin_voices", {
        ...(settings?.plugin_voices ?? {}),
        [plugin.id]: voiceId,
      });
    }
    const installed = plugin.setup_status?.voices ?? [];
    if (installed.includes(voiceId)) {
      // 后台预加载（秒级），失败不拦截——合成时还会幂等补齐
      setPreloadingVoice(voiceId);
      preloadVoice(plugin.id, voiceId)
        .catch(() => {})
        .finally(() => setPreloadingVoice((v) => (v === voiceId ? null : v)));
      return patch("plugin_voices", {
        ...(settings?.plugin_voices ?? {}),
        [plugin.id]: voiceId,
      });
    }
    // 未安装：有任务在跑就不开新确认（入口禁用语义）
    if (taskRunning) return;
    setPendingVoice({ pluginId: plugin.id, voiceId, label: voiceLabel(plugin, voiceId) });
  }

  /** 确认卡片：确认下载音色 → 启动安装任务，成功后才切换音色 */
  function confirmVoiceDownload() {
    if (!pendingVoice) return;
    const { pluginId, voiceId, label } = pendingVoice;
    setPendingVoice(null);
    startVoice(pluginId, voiceId, label).then(
      () => {
        void patch("plugin_voices", {
          ...(settings?.plugin_voices ?? {}),
          [pluginId]: voiceId,
        });
        listPlugins().then(setPlugins).catch(() => {});
      },
      () => {
        /* 错误已记录在 store，进度面板展示 */
      },
    );
  }

  return (
    <div className="space-y-3">
      {/* 默认引擎选择 */}
      <Field label="默认 TTS 引擎">
        <select
          value={engineId}
          onChange={(e) => handleEngineChange(e.target.value)}
          className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
        >
          {plugins
            .filter((p) => p.loaded && p.plugin_type === "tts_engine")
            .map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
        </select>
      </Field>

      {/* ── 插件引擎配置（音色表来自插件本身，通用） ── */}
      {(() => {
        const cur = plugins.find((p) => p.id === engineId && p.loaded);
        if (!cur) {
          // 设置或插件列表还没到位就不下结论：plugins 初值是空数组、listPlugins 是异步的，
          // 直接往下判定会让每次进页的首帧都闪一张「插件未安装」红卡
          if (!engineId || plugins.length === 0) return null;
          // 当前引擎是插件但不可用：未安装（卸载/换机）或加载失败。给警示卡片 + 一键切到可用引擎
          const failed = plugins.find((p) => p.id === engineId);
          // 与宿主卸载兜底（commands/plugins.rs 的 pick_fallback_engine）同口径：
          // 已加载 + tts_engine + 环境已就绪，优先免必填配置，同优先级按 id 字典序。
          // 不往这里贴任何引擎名（基座合同）。
          const usable = (p: PluginInfo) =>
            p.loaded && p.plugin_type === "tts_engine" && (!p.has_setup || p.setup_status?.ready === true);
          // 「缺必填」要同时问「用户填了没有」，与宿主 missing_required_config 同口径：
          // 只看「声明过必填」会把已填好 Key 的引擎错排到后面
          const missingKey = (p: PluginInfo) =>
            !!p.config?.fields.some(
              (f) => f.required && !(settings?.plugin_config?.[p.id]?.[f.key] ?? "").trim(),
            );
          const alt = plugins
            .filter(usable)
            .sort((a, b) => Number(missingKey(a)) - Number(missingKey(b)) || (a.id < b.id ? -1 : 1))[0];
          return (
            <div className="rounded-lg border border-[var(--seal)]/40 bg-[var(--seal)]/10 px-3 py-2.5">
              <p className="text-xs font-medium text-[var(--seal)]">
                {failed ? `当前引擎「${failed.name}」加载失败` : "当前引擎对应的插件未安装"}
              </p>
              <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-500)]">
                {failed
                  ? `原因：${failed.error ?? "未知"}。可到插件管理页查看详情，或先切换到其他引擎。`
                  : `配置中记录的引擎「${engineId}」对应的插件尚未安装（可能已卸载或在别的设备上安装）。语音合成暂不可用，可先切到其他引擎，或到插件管理页安装。`}
              </p>
              {alt && (
                <button
                  onClick={() => handleEngineChange(alt.id)}
                  className="mt-2 rounded-lg bg-[var(--ink-900)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)]"
                >
                  切换到「{alt.name}」
                </button>
              )}
            </div>
          );
        }

        // 当前音色下拉：MiniMax 国际版并入「音色管理」卡内（经 voiceSelect 插槽），其余引擎就地渲染
        const voiceSelectNode = (
          <Field label="当前音色">
            <select
              value={settings?.plugin_voices?.[cur.id] ?? cur.voices[0]?.id ?? ""}
              onChange={(e) => handlePluginVoiceChange(cur, e.target.value)}
              disabled={taskRunning}
              className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)] disabled:opacity-60"
            >
              {cur.voices.map((v) => (
                <option key={v.id} value={v.id}>{v.label}</option>
              ))}
              {/* MiniMax（国际版/国内流式版）：合并本地持久化的克隆音色（首次合成前账号列表查不到） */}
              {(cur.id === "minimax-tts-global" || cur.id === "minimax-tts-stream") &&
                (settings?.minimax_global_cloned_voices ?? [])
                  .filter((id) => !cur.voices.some((v) => v.id === id))
                  .map((id) => (
                    <option key={`clone:${id}`} value={id}>克隆·{id}</option>
                  ))}
            </select>
            {preloadingVoice !== null && (
              <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-[var(--ink-500)]">
                <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-[var(--amber-500)]" />
                正在加载音色…
              </div>
            )}
          </Field>
        );

        return (
          <EngineCard
            kind="tts"
            name={cur.name}
            version={cur.version}
            category={cur.category}
            loaded={cur.loaded}
            error={cur.error}
          >
            {cur.id === "edge-tts" && (
              <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
                Edge TTS 为<b>免费</b>引擎（微软 Edge 朗读服务），无需 API Key。
                但它是非官方接口，可能不稳定或受地区限制（部分地区 403），失效不属于软件 bug。
              </div>
            )}

            {/* 通用插件配置卡（manifest 声明驱动，保存即生效）：标题统一为「API 密钥」 */}
            {cur.config && <PluginConfigPanel pluginId={cur.id} pluginName={cur.name} title="API 密钥" />}

            {/* 音色管理区（含「当前音色」下拉）：MiniMax 国际版/国内流式版走 MinimaxVoicePanel（克隆+账号管理），
                本地引擎并入 VoiceManager（装/卸/导入音色包），其余云引擎用「音色管理」分区卡 */}
            {cur.id === "minimax-tts-global" || cur.id === "minimax-tts-stream" ? (
              <MinimaxVoicePanel plugin={cur} voiceSelect={voiceSelectNode} />
            ) : cur.has_voice_management ? (
              <VoiceManager
                plugin={cur}
                currentVoiceId={settings?.plugin_voices?.[cur.id] ?? cur.voices[0]?.id ?? ""}
                onChanged={() => {
                  listPlugins().then(setPlugins).catch(() => {});
                }}
                voiceSelect={voiceSelectNode}
              />
            ) : (
              /* 其余云引擎用「音色管理」分区卡。「我的音色」按能力显示（B5）：读插件自述的
                 manifest capabilities.voice_library，缺省视为 false —— 没有云端音色库的引擎
                 不再摆一个用户照着填也永远用不上的音色保存框；既无内置音色又无音色库时整块不渲染，
                 不留空壳 */
              !hasVoiceLibrary(cur) && cur.voices.length === 0 ? null : (
              <SubPanel
                title="音色管理"
                // 文案按插件自述的音色表与能力位决定，不认引擎身份
                desc={
                  !hasVoiceLibrary(cur)
                    ? "选择引擎内置音色。"
                    : cur.voices.length > 0
                      ? "选择引擎内置音色，或维护你自己的「我的音色」（如克隆音色 ID）。"
                      : "该引擎不自带音色：请在下方「我的音色」保存音色 ID，再回这里选用。"
                }
              >
                <Field label="当前音色">
                  <select
                    value={settings?.plugin_voices?.[cur.id] ?? cur.voices[0]?.id ?? ""}
                    onChange={(e) => handlePluginVoiceChange(cur, e.target.value)}
                    disabled={taskRunning}
                    className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)] disabled:opacity-60"
                  >
                    {/* 用户裁定（10-23）：保存的克隆音色排列表顶部，对所有走通用下拉的插件生效 */}
                    {(settings?.plugin_custom_voices?.[cur.id] ?? []).length > 0 && (
                      <optgroup label="我的音色">
                        {(settings?.plugin_custom_voices?.[cur.id] ?? []).map((cv) => (
                          <option key={`custom:${cv.id}`} value={cv.id}>
                            {cv.label}
                          </option>
                        ))}
                      </optgroup>
                    )}
                    {cur.voices.length > 0 && (
                      <optgroup label="内置音色">
                        {cur.voices.map((v) => (
                          <option key={v.id} value={v.id}>{v.label}</option>
                        ))}
                      </optgroup>
                    )}
                  </select>
                </Field>
                {hasVoiceLibrary(cur) && (
                <CustomVoicePanel
                  items={settings?.plugin_custom_voices?.[cur.id] ?? []}
                  currentVoiceId={settings?.plugin_voices?.[cur.id] ?? cur.voices[0]?.id ?? ""}
                  onSelect={(vid) => handlePluginVoiceChange(cur, vid)}
                  onSave={(cv) =>
                    void patch("plugin_custom_voices", {
                      ...(settings?.plugin_custom_voices ?? {}),
                      [cur.id]: [...(settings?.plugin_custom_voices?.[cur.id] ?? []), cv],
                    })
                  }
                  onDelete={(vid) => {
                    void patch("plugin_custom_voices", {
                      ...(settings?.plugin_custom_voices ?? {}),
                      [cur.id]: (settings?.plugin_custom_voices?.[cur.id] ?? []).filter(
                        (x) => x.id !== vid,
                      ),
                    });
                    // 正选中被删条目：去掉选择键，自然回退到首个内置音色
                    if (settings?.plugin_voices?.[cur.id] === vid) {
                      const pv = { ...(settings?.plugin_voices ?? {}) };
                      delete pv[cur.id];
                      void patch("plugin_voices", pv);
                    }
                  }}
                />
                )}
              </SubPanel>
              )
            )}

            {/* 克隆样本卡：插件自述驱动——`voices` 里有 id="clone" 就显示，
                不往任何硬编码集合里加引擎名（基座合同 §3.6-1） */}
            {(() => {
              const cloneVoice = cur.voices.find((v) => v.id === "clone");
              if (!cloneVoice) return null;
              const usingClone = (settings?.plugin_voices?.[cur.id] ?? "") === cloneVoice.id;
              return (
                <CloneSamplePanel
                  selected={usingClone}
                  onImported={() => handlePluginVoiceChange(cur, cloneVoice.id)}
                  /* 样本没了 = 克隆音色当下不可用，得把选择退回去。
                     宿主 remove_clone_voice 只管样本文件，认不出「哪个音色依赖样本」
                    （那是插件 voices 的自述），只有这里能判；不退回的话用户每次合成
                     都会撞到插件侧「未导入克隆音色样本」的错 */
                  onRemoved={() => {
                    if (!usingClone) return;
                    const next = cur.voices.find((v) => v.id !== cloneVoice.id);
                    if (next) return handlePluginVoiceChange(cur, next.id);
                  }}
                />
              );
            })()}

            {/* 未安装音色：页内确认卡片（确认前不改配置） */}
            {pendingVoice && pendingVoice.pluginId === cur.id && (
              <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/15 px-3 py-2.5">
                <p className="text-[11px] leading-relaxed text-[var(--ink-700)]">
                  音色「{pendingVoice.label}」尚未下载（约 200MB，需联网）。现在下载吗？
                </p>
                <div className="mt-2 flex gap-2">
                  <button
                    onClick={confirmVoiceDownload}
                    className="rounded-lg bg-[var(--amber-500)] px-3 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90"
                  >
                    下载并切换
                  </button>
                  <button
                    onClick={() => setPendingVoice(null)}
                    className="rounded-lg border border-[var(--ink-200)] px-3 py-1 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
                  >
                    取消
                  </button>
                </div>
              </div>
            )}

            {/* 安装进度面板（纯订阅 store；无匹配任务时自动隐藏） */}
            <PluginSetupPanel
              pluginId={cur.id}
              onClosed={() => {
                listPlugins().then(setPlugins).catch(() => {});
              }}
            />
          </EngineCard>
        );
      })()}

    </div>
  );
}
