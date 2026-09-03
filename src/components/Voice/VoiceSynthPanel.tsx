// 语音中心 · TTS 合成面板：默认引擎选择 + 逐引擎配置（MiMo 内置 / MOSS 内置 / 插件引擎）。
// 从设置页「语音合成」区整体析出，行为保持一致；MiniMax 国际克隆/管理交由 MinimaxVoicePanel 承担。
// 引擎以 EngineCard 同构呈现；本地引擎环境未就绪时仍走页内确认卡（在线下载 / 导入离线资源包），与原实现一致。

import { useState, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import {
  listPlugins,
  importCloneVoice,
  removeCloneVoice,
  pickAudioFile,
  preloadVoice,
  promptEngineWarmup,
  importResourcePackFlow,
  getRemoteConfig,
} from "../../services/invoke";
import type { PluginInfo, MossVoice } from "../../types";
import { Field, SubPanel, SecretInput } from "../common/SettingsSection";
import { EngineCard } from "./EngineCard";
import { MinimaxVoicePanel } from "./MinimaxVoicePanel";
import { PluginSetupPanel } from "../Plugins/PluginSetupPanel";
import { PluginConfigPanel } from "../Settings/PluginConfigPanel";
import { ResourcePackLinks } from "../Plugins/ResourcePackLinks";
import { VoiceManager } from "../Settings/VoiceManager";

const PRESET_VOICES = [
  { id: "mimo_default", label: "默认 (mimo_default)" },
  { id: "冰糖", label: "冰糖（女声）" },
  { id: "茉莉", label: "茉莉（女声）" },
  { id: "苏打", label: "苏打（男声）" },
  { id: "白桦", label: "白桦（男声）" },
];

export function VoiceSynthPanel() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const setSettings = useSettingsStore((s) => s.setSettings);

  // MiMo 克隆/邀请
  const [importing, setImporting] = useState(false);
  const [cloneName, setCloneName] = useState(settings?.clone_voice_name ?? "");
  const [copied, setCopied] = useState(false);
  // MiMo 邀请码：远程配置动态下发（后端 24h 缓存，断网退回缓存/内置默认值）
  // 初值用当前有效码（与内置兜底/在线值一致），拉取成功后被覆盖；避免拉取前闪现旧失效码
  const [inviteCode, setInviteCode] = useState("5P9J2B");
  useEffect(() => {
    getRemoteConfig()
      .then((c) => setInviteCode(c.mimo_invite_code))
      .catch(() => {});
  }, []);

  // MOSS 音色库管理
  const [addName, setAddName] = useState("");
  const [addId, setAddId] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editId, setEditId] = useState("");

  // 已加载插件（引擎下拉动态项 + 插件音色来源）
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  useEffect(() => {
    listPlugins().then(setPlugins).catch(() => {});
  }, []);

  // 安装任务走全局 store（启动在用户确认处，进度面板只订阅）
  const task = usePluginTaskStore((s) => s.task);
  const startEnv = usePluginTaskStore((s) => s.startEnv);
  const startVoice = usePluginTaskStore((s) => s.startVoice);
  const taskRunning = task?.status === "running";

  // 待确认项（页内确认卡片，替代 window.confirm，且确认前不改配置）
  const [pendingEnv, setPendingEnv] = useState<{ pluginId: string; name: string } | null>(null);
  const [pendingVoice, setPendingVoice] = useState<
    { pluginId: string; voiceId: string; label: string } | null
  >(null);
  // 正在预加载的音色（切换已装音色时的瞬时指示）
  const [preloadingVoice, setPreloadingVoice] = useState<string | null>(null);

  const currentVoice = settings?.tts_model ?? "mimo_default";
  const mossVoices = settings?.moss_voices ?? [];
  const engineId = settings?.tts_engine ?? "mimo";

  /** 取插件音色的干净展示名（去掉"· 待下载"后缀） */
  function voiceLabel(plugin: PluginInfo, voiceId: string): string {
    const v = plugin.voices.find((x) => x.id === voiceId);
    return (v?.label ?? voiceId).replace(/\s*·\s*待下载$/, "");
  }

  /** 切换引擎：选中未就绪的本地插件时，用页内卡片询问是否现在下载运行环境；
   *  环境已就绪的本地引擎则询问是否后台预热（避免首次对话长等待） */
  function handleEngineChange(newId: string) {
    void patch("tts_engine", newId);
    setPendingEnv(null);
    const p = plugins.find((x) => x.id === newId);
    if (p?.has_setup && !p.setup_status?.ready) {
      setPendingEnv({ pluginId: newId, name: p.name });
      return;
    }
    if (p?.category === "local" && p.setup_status?.ready) {
      const vid = settings?.plugin_voices?.[p.id] ?? p.voices[0]?.id ?? "";
      if (vid) void promptEngineWarmup(p.name, p.id, vid);
    }
  }

  /** 切换插件音色：
   *  - 已安装 → 立即切换 + 后台预加载（有瞬时指示）；
   *  - 未安装 → 先弹页内确认卡片，确认并【安装成功后】才切换（确认前不动配置）。 */
  function handlePluginVoiceChange(plugin: PluginInfo, voiceId: string) {
    // 无音色管理能力的插件（云端引擎）：音色即开即用，直接切换
    if (!plugin.has_voice_management) {
      void patch("plugin_voices", {
        ...(settings?.plugin_voices ?? {}),
        [plugin.id]: voiceId,
      });
      return;
    }
    const installed = plugin.setup_status?.voices ?? [];
    if (installed.includes(voiceId)) {
      void patch("plugin_voices", {
        ...(settings?.plugin_voices ?? {}),
        [plugin.id]: voiceId,
      });
      // 后台预加载（秒级），失败不拦截——合成时还会幂等补齐
      setPreloadingVoice(voiceId);
      preloadVoice(plugin.id, voiceId)
        .catch(() => {})
        .finally(() => setPreloadingVoice((v) => (v === voiceId ? null : v)));
      return;
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

  /** 确认卡片：确认下载引擎运行环境 → 启动环境安装任务 */
  function confirmEnvDownload() {
    if (!pendingEnv) return;
    const { pluginId, name } = pendingEnv;
    setPendingEnv(null);
    startEnv(pluginId, name).then(
      () => {
        listPlugins().then(setPlugins).catch(() => {});
      },
      () => {
        /* 错误已记录在 store */
      },
    );
  }

  /** 确认卡片：导入离线资源包（网盘/QQ 群下载的 zip） */
  async function confirmEnvImport() {
    if (!pendingEnv) return;
    const { pluginId } = pendingEnv;
    setPendingEnv(null);
    try {
      const done = await importResourcePackFlow(pluginId);
      if (done) listPlugins().then(setPlugins).catch(() => {});
    } catch (e) {
      window.alert(`导入资源包失败：${e}`);
    }
  }

  async function copyInvite() {
    try {
      await navigator.clipboard.writeText(inviteCode);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = inviteCode;
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); setCopied(true); setTimeout(() => setCopied(false), 1600); } catch {}
      document.body.removeChild(ta);
    }
  }

  // MOSS 音色库 CRUD
  async function saveVoiceList(list: MossVoice[]) {
    await patch("moss_voices", list);
  }
  async function addVoice() {
    const name = addName.trim();
    const id = addId.trim();
    if (!name || !id) return;
    await saveVoiceList([...mossVoices, { name, voice_id: id }]);
    setAddName("");
    setAddId("");
  }
  async function removeVoice(voiceId: string) {
    const list = mossVoices.filter((v) => v.voice_id !== voiceId);
    await saveVoiceList(list);
    if (settings?.moss_voice_id === voiceId) {
      await patch("moss_voice_id", list[0]?.voice_id ?? "");
    }
    if (editingId === voiceId) setEditingId(null);
  }
  function startEdit(v: MossVoice) {
    setEditingId(v.voice_id);
    setEditName(v.name);
    setEditId(v.voice_id);
  }
  async function saveEdit() {
    if (editingId === null) return;
    const name = editName.trim();
    const id = editId.trim();
    if (!name || !id) return;
    const list = mossVoices.map((v) =>
      v.voice_id === editingId ? { name, voice_id: id } : v,
    );
    await saveVoiceList(list);
    if (settings?.moss_voice_id === editingId && id !== editingId) {
      await patch("moss_voice_id", id);
    }
    setEditingId(null);
  }

  // MiMo 预置/克隆音色
  async function onPickVoice(v: string) {
    if (v !== "clone") {
      await patch("tts_model", v);
    }
  }
  async function onImportClone() {
    const filePath = await pickAudioFile();
    if (!filePath) return;
    const name = window.prompt("给这个克隆音色起个名字：", cloneName || "我的音色");
    if (!name || !name.trim()) return;
    setImporting(true);
    try {
      await importCloneVoice(filePath, name.trim());
      setCloneName(name.trim());
      await patch("tts_model", "clone");
      const { getSettings } = await import("../../services/invoke");
      setSettings(await getSettings());
    } catch (e) {
      window.alert(`导入克隆样本失败：${e}`);
    } finally {
      setImporting(false);
    }
  }
  async function onRemoveClone() {
    if (!confirm("确认删除克隆音色样本？将切回默认音色。")) return;
    try {
      await removeCloneVoice();
      await patch("tts_model", "mimo_default");
      setCloneName("");
      const { getSettings } = await import("../../services/invoke");
      setSettings(await getSettings());
    } catch (e) {
      window.alert(`删除失败：${e}`);
    }
  }
  async function onSaveApiKey(v: string) {
    await patch("mimo_api_key", v.trim());
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
          <option value="mimo">MiMo（小米）</option>
          <option value="moss">Moss-TTS（Mossland）</option>
          {plugins
            .filter((p) => p.loaded && p.plugin_type === "tts_engine")
            .map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
        </select>
      </Field>

      {/* ── MiMo 内置引擎 ── */}
      {engineId === "mimo" && (
        <EngineCard kind="tts" name="MiMo（小米）" category="remote">
          <SubPanel
            title="API 密钥"
            desc="填入小米 MiMo 平台的 API Key 用于云端合成。"
            right={
              <a
                href={`https://platform.xiaomimimo.com?ref=${inviteCode}`}
                target="_blank"
                rel="noreferrer"
                className="rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
              >
                获取 API Key ↗
              </a>
            }
          >
            <SecretInput
              value={settings?.mimo_api_key ?? ""}
              onCommit={onSaveApiKey}
              placeholder="sk-..."
            />
            <div className="mt-2 flex items-center gap-2 text-xs text-[var(--ink-500)]">
              <span>填写邀请码</span>
              <button
                onClick={copyInvite}
                title="点击复制邀请码"
                className={[
                  "inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 font-mono tracking-wider transition-all",
                  copied
                    ? "border-[var(--amber-500)] bg-[var(--amber-200)]/40 text-[var(--amber-600)]"
                    : "border-[var(--ink-200)] bg-[var(--paper-card)] text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]",
                ].join(" ")}
              >
                {inviteCode}
                <span className={["text-[10px]", copied ? "text-[var(--amber-600)]" : "text-[var(--ink-300)]"].join(" ")}>
                  {copied ? "✓ 已复制" : "⧉"}
                </span>
              </button>
              <span>获得 10R 额度</span>
            </div>
          </SubPanel>

          <SubPanel title="音色管理" desc="选择默认朗读音色。">
            <select
              value={currentVoice}
              onChange={(e) => onPickVoice(e.target.value)}
              className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
            >
              {PRESET_VOICES.map((v) => (
                <option key={v.id} value={v.id}>{v.label}</option>
              ))}
              {settings?.clone_voice_path && (
                <option value="clone">
                  克隆：{settings?.clone_voice_name || "未命名"}
                </option>
              )}
            </select>
          </SubPanel>

          <SubPanel
            title="音色克隆"
            desc="导入一段 5–10 秒的本地说话音频（mp3/wav，≤10MB），MiMo 会用它合成相似音色。每次合成都要把样本传给 MiMo，速度比预置音色慢。"
          >
            {settings?.clone_voice_path ? (
              <div className="space-y-2">
                <div className="rounded-xl border border-[var(--ink-200)] bg-[var(--ink-100)]/50 px-3 py-2 text-xs text-[var(--ink-500)]">
                  当前样本：<span className="font-medium text-[var(--ink-900)]">{settings.clone_voice_name || "未命名"}</span>
                </div>
                <button
                  onClick={onImportClone}
                  disabled={importing}
                  className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-xs text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] transition-colors disabled:opacity-50"
                >
                  {importing ? "导入中…" : "替换样本"}
                </button>
                <button
                  onClick={onRemoveClone}
                  className="w-full rounded-xl px-3 py-2 text-xs text-[var(--seal)] hover:bg-[var(--seal)]/10 transition-colors"
                >
                  删除克隆样本
                </button>
              </div>
            ) : (
              <button
                onClick={onImportClone}
                disabled={importing}
                className="w-full rounded-xl bg-[var(--ink-900)] px-3 py-2 text-xs text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:opacity-40"
              >
                {importing ? "导入中…" : "+ 导入样本"}
              </button>
            )}
          </SubPanel>
        </EngineCard>
      )}

      {/* ── MOSS 内置引擎 ── */}
      {engineId === "moss" && (
        <EngineCard kind="tts" name="Moss-TTS（Mossland）" category="remote">
          <SubPanel
            title="API 密钥"
            desc="填入 Mossland 控制台 API Key 用于云端合成。"
            right={
              <a
                href="https://platform.mosi.cn/app/api-keys"
                target="_blank"
                rel="noreferrer"
                className="rounded-lg border border-[var(--ink-200)] px-2 py-1 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
              >
                获取 API Key ↗
              </a>
            }
          >
            <SecretInput
              value={settings?.moss_api_key ?? ""}
              onCommit={(v) => patch("moss_api_key", v.trim())}
              placeholder="sk-..."
            />
          </SubPanel>

          <SubPanel title="音色管理" desc="选择当前使用音色，或维护你的音色库。">
            <select
              value={settings?.moss_voice_id ?? ""}
              onChange={(e) => patch("moss_voice_id", e.target.value)}
              className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
            >
              {mossVoices.length === 0 && <option value="">（暂无音色，请先在下方添加）</option>}
              {mossVoices.map((v) => (
                <option key={v.voice_id} value={v.voice_id}>{v.name}</option>
              ))}
            </select>
            <div className="mt-2.5 space-y-1.5">
              {mossVoices.map((v) => (
                editingId === v.voice_id ? (
                  <div key={v.voice_id} className="space-y-1.5 rounded-xl border border-[var(--amber-500)] bg-[var(--amber-200)]/15 p-2">
                    <input
                      type="text"
                      value={editName}
                      onChange={(e) => setEditName(e.target.value)}
                      placeholder="音色名称"
                      className="w-full rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--amber-500)]"
                    />
                    <input
                      type="text"
                      value={editId}
                      onChange={(e) => setEditId(e.target.value)}
                      placeholder="音色 id"
                      className="w-full rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--amber-500)] font-mono"
                    />
                    <div className="flex gap-1.5">
                      <button onClick={saveEdit} className="flex-1 rounded-lg bg-[var(--ink-900)] px-2 py-1 text-xs text-[var(--paper)] hover:bg-[var(--ink-700)] transition-colors">保存</button>
                      <button onClick={() => setEditingId(null)} className="flex-1 rounded-lg border border-[var(--ink-200)] px-2 py-1 text-xs text-[var(--ink-700)] hover:bg-[var(--ink-100)] transition-colors">取消</button>
                    </div>
                  </div>
                ) : (
                  <div key={v.voice_id} className="flex items-center justify-between rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2">
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-xs font-medium text-[var(--ink-900)]">{v.name}</div>
                      <div className="truncate text-[10px] text-[var(--ink-300)] font-mono">{v.voice_id}</div>
                    </div>
                    <div className="ml-2 flex shrink-0 gap-1">
                      <button onClick={() => startEdit(v)} title="编辑" className="rounded-md px-1.5 py-1 text-xs text-[var(--ink-500)] hover:bg-[var(--ink-100)] hover:text-[var(--ink-900)] transition-colors">✎</button>
                      <button onClick={() => removeVoice(v.voice_id)} title="删除" className="rounded-md px-1.5 py-1 text-xs text-[var(--ink-500)] hover:bg-[var(--seal)]/10 hover:text-[var(--seal)] transition-colors">×</button>
                    </div>
                  </div>
                )
              ))}
            </div>
            <div className="mt-2.5 space-y-1.5 rounded-xl border border-dashed border-[var(--ink-200)] p-2.5">
              <input
                type="text"
                value={addName}
                onChange={(e) => setAddName(e.target.value)}
                placeholder="音色名称（如 曼波）"
                className="w-full rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--amber-500)]"
              />
              <input
                type="text"
                value={addId}
                onChange={(e) => setAddId(e.target.value)}
                placeholder="音色 id（从音色库复制）"
                className="w-full rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--amber-500)] font-mono"
              />
              <button
                onClick={addVoice}
                disabled={!addName.trim() || !addId.trim()}
                className="w-full rounded-lg bg-[var(--ink-900)] px-2 py-1.5 text-xs text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:opacity-40"
              >
                + 添加音色
              </button>
            </div>
            <a
              href="https://mossland.mosi.cn/voice/library"
              target="_blank"
              rel="noreferrer"
              className="mt-2 inline-block text-xs text-[var(--ink-500)] underline underline-offset-2 hover:text-[var(--amber-600)] transition-colors"
            >
              前往 Mossland 音色库查询 voice_id
            </a>
          </SubPanel>
        </EngineCard>
      )}

      {/* ── 插件引擎配置（音色表来自插件本身，通用） ── */}
      {engineId !== "mimo" && engineId !== "moss" && (() => {
        const cur = plugins.find((p) => p.id === engineId && p.loaded);
        if (!cur) {
          // 当前引擎是插件但不可用：未安装（卸载/换机）或加载失败。给警示卡片 + 一键切回内置引擎
          const failed = plugins.find((p) => p.id === engineId);
          return (
            <div className="rounded-lg border border-[var(--seal)]/40 bg-[var(--seal)]/10 px-3 py-2.5">
              <p className="text-xs font-medium text-[var(--seal)]">
                {failed ? `当前引擎「${failed.name}」加载失败` : "当前引擎对应的插件未安装"}
              </p>
              <p className="mt-1 text-[11px] leading-relaxed text-[var(--ink-500)]">
                {failed
                  ? `原因：${failed.error ?? "未知"}。可到插件管理页查看详情，或先切换到其他引擎。`
                  : `配置中记录的引擎「${engineId}」对应的插件尚未安装（可能已卸载或在别的设备上安装）。语音合成暂不可用，可先切回内置引擎，或到插件管理页安装。`}
              </p>
              <button
                onClick={() => handleEngineChange("mimo")}
                className="mt-2 rounded-lg bg-[var(--ink-900)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)]"
              >
                切换回 MiMo 引擎
              </button>
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
              {/* 国际版：合并本地持久化的克隆音色（首次合成前账号列表查不到） */}
              {cur.id === "minimax-tts-global" &&
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

            {/* 国内版阉割提示：暂不提供音色克隆 */}
            {cur.id === "minimax-tts" && (
              <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
                国内版为<b>阉割版</b>，暂不提供音色克隆。如需克隆自己的音色，请安装并使用 MiniMax 国际版插件。
              </div>
            )}

            {/* 音色管理区（含「当前音色」下拉）：国际版走 MinimaxVoicePanel（克隆+账号管理），
                本地引擎并入 VoiceManager（装/卸/导入音色包），其余云引擎用「音色管理」分区卡 */}
            {cur.id === "minimax-tts-global" ? (
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
              <SubPanel title="音色管理">{voiceSelectNode}</SubPanel>
            )}

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

            {/* 引擎环境未就绪：页内确认卡片 */}
            {pendingEnv && pendingEnv.pluginId === cur.id && (
              <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/15 px-3 py-2.5">
                <p className="text-[11px] leading-relaxed text-[var(--ink-700)]">
                  「{pendingEnv.name}」是本地引擎，首次使用需安装运行环境与语音模型（共约 1.1GB）。
                  在线下载源在 HuggingFace，国内网络需开启代理（魔法上网）；
                  也可从<ResourcePackLinks />下载离线资源包（genie-resources-v1.zip，约 800MB）直接导入。
                </p>
                {cur.requirements && (
                  <p className="mt-1.5 text-[10px] leading-relaxed text-[var(--ink-500)]">
                    资源需求：{cur.requirements}
                  </p>
                )}
                <div className="mt-2 flex gap-2">
                  <button
                    onClick={confirmEnvDownload}
                    className="rounded-lg bg-[var(--amber-500)] px-3 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90"
                  >
                    在线下载（需魔法上网）
                  </button>
                  <button
                    onClick={() => void confirmEnvImport()}
                    className="rounded-lg border border-[var(--amber-500)] px-3 py-1 text-[11px] font-medium text-[var(--amber-600)] transition-colors hover:bg-[var(--amber-500)]/10"
                  >
                    导入离线资源包
                  </button>
                  <button
                    onClick={() => setPendingEnv(null)}
                    className="rounded-lg border border-[var(--ink-200)] px-3 py-1 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
                  >
                    稍后
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
