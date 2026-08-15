// 设置页面（主界面侧边栏"设置"项的右侧内容区）：分类收纳（手风琴），消除平铺卡顿。
// 分类：语音合成 / 虚拟麦克风 / 语音输入（仅装了 ASR 插件时显示） / 快捷键 / 外观 / 关于。
// 虚拟麦克风轮询隔离在 MicSettings 组件，分类收起即停止轮询。

import { useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSettingsStore } from "../../stores/settingsStore";
import { useUpdateStore, shouldShowUpdateDot } from "../../stores/updateStore";
import { usePluginTaskStore } from "../../stores/pluginTaskStore";
import { importCloneVoice, removeCloneVoice, pickAudioFile, listPlugins, listAsrPlugins, setHotkey, setVoiceInputHotkey, setPlayLastHotkey, setMicToggleHotkey, preloadVoice, importResourcePackFlow, promptEngineWarmup, getRemoteConfig, minimaxGlobalVoiceClone, minimaxGlobalGetVoices, minimaxGlobalDeleteVoice } from "../../services/invoke";
import type { MossVoice, PluginInfo } from "../../types";
import { HotkeyRecorder } from "./HotkeyRecorder";
import { MicSettings } from "./MicSettings";
import { VoiceInputSettings } from "./VoiceInputSettings";
import { PluginSetupPanel } from "../Plugins/PluginSetupPanel";
import { ResourcePackLinks } from "../Plugins/ResourcePackLinks";
import { VoiceManager } from "./VoiceManager";

const PRESET_VOICES = [
  { id: "mimo_default", label: "默认 (mimo_default)" },
  { id: "冰糖", label: "冰糖（女声）" },
  { id: "茉莉", label: "茉莉（女声）" },
  { id: "苏打", label: "苏打（男声）" },
  { id: "白桦", label: "白桦（男声）" },
];

const THEMES = [
  { id: "light", label: "安墨（浅色）", desc: "宣纸暖白 · 墨色 · 暖琥珀" },
  { id: "dark",  label: "夜窗（深色）", desc: "深炭灰 · 琥珀高光 · 夜间友好" },
] as const;

// MiniMax 国际版克隆/音色管理 API 端点（T2A 由插件走 api-uw 加速端点）
const MM_GLOBAL_BASE = "https://api.minimax.io";

/** get_voice 返回的音色条目（克隆/设计组） */
interface MmAccountVoice {
  voice_id: string;
  voice_name?: string;
  created_time?: string;
}

export function SettingsPage() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const setSettings = useSettingsStore((s) => s.setSettings);

  // 版本更新：关于区红点 + 新版本入口
  const updateLatest = useUpdateStore((s) => s.latest);
  const updateChecked = useUpdateStore((s) => s.checked);
  const checkUpdate = useUpdateStore((s) => s.check);
  const resetDialog = useUpdateStore((s) => s.resetDialog);
  const updateDot = useUpdateStore(shouldShowUpdateDot);
  const markAboutSeen = useUpdateStore((s) => s.markAboutSeen);

  // 关于：当前版本号 + 手动检查更新
  const [appVersion, setAppVersion] = useState<string>("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
  }, []);

  // 「语音输入」分类仅在安装了可用 ASR 插件时显示（与插件绑定，非本体功能）
  const [asrAvailable, setAsrAvailable] = useState(false);
  useEffect(() => {
    listAsrPlugins()
      .then((ps) => setAsrAvailable(ps.some((p) => p.loaded)))
      .catch(() => {});
  }, []);
  async function handleCheckUpdate() {
    setCheckingUpdate(true);
    try {
      await checkUpdate();
      // 手动检查后重置弹窗状态，使新版本弹窗可以再次弹出
      resetDialog();
    } finally {
      setCheckingUpdate(false);
    }
  }
  const [importing, setImporting] = useState(false);
  const [cloneName, setCloneName] = useState(settings?.clone_voice_name ?? "");
  const [copied, setCopied] = useState(false);

  // MiMo 邀请码：远程配置动态下发（后端 24h 缓存，断网退回缓存/内置默认值）
  const [inviteCode, setInviteCode] = useState("U277DH");
  useEffect(() => {
    getRemoteConfig()
      .then((c) => setInviteCode(c.mimo_invite_code))
      .catch(() => {});
  }, []);

  // 音色库管理状态
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

  // MiniMax 音色克隆（仅国际版：文件/voice_id/高级选项/进行中/结果提示）
  const [mmCloneFile, setMmCloneFile] = useState("");
  const [mmCloneVoiceId, setMmCloneVoiceId] = useState("");
  const [mmShowAdvanced, setMmShowAdvanced] = useState(false);
  const [mmPromptFile, setMmPromptFile] = useState("");
  const [mmPromptText, setMmPromptText] = useState("");
  const [mmCloning, setMmCloning] = useState(false);
  const [mmCloneMsg, setMmCloneMsg] = useState<{ ok: boolean; text: string } | null>(null);

  // MiniMax 音色管理（账号音色查询/删除）
  const [mmAccountVoices, setMmAccountVoices] = useState<{
    cloning: MmAccountVoice[];
    generation: MmAccountVoice[];
    systemCount: number;
  } | null>(null);
  const [mmVoicesLoading, setMmVoicesLoading] = useState(false);
  const [mmManageMsg, setMmManageMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [mmDeleteTarget, setMmDeleteTarget] = useState<{ type: string; id: string } | null>(null);

  /** 取插件音色的干净展示名（去掉"· 待下载"后缀） */
  function voiceLabel(plugin: PluginInfo, voiceId: string): string {
    const v = plugin.voices.find((x) => x.id === voiceId);
    return (v?.label ?? voiceId).replace(/\s*·\s*待下载$/, "");
  }

  /** 切换引擎：选中未就绪的本地插件时，用页内卡片询问是否现在下载运行环境；
   *  环境已就绪的本地引擎则询问是否后台预热（避免首次对话长等待） */
  function handleEngineChange(engineId: string) {
    void patch("tts_engine", engineId);
    setPendingEnv(null);
    const p = plugins.find((x) => x.id === engineId);
    if (p?.has_setup && !p.setup_status?.ready) {
      setPendingEnv({ pluginId: engineId, name: p.name });
      return;
    }
    if (p?.category === "local" && p.setup_status?.ready) {
      const voiceId = settings?.plugin_voices?.[p.id] ?? p.voices[0]?.id ?? "";
      if (voiceId) void promptEngineWarmup(p.name, p.id, voiceId);
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
        // 安装成功 → 切到该音色 + 刷新插件状态
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

  /** MiniMax 国际版音色克隆：上传音频（+可选样本）→ voice_clone → 持久化并切换 */
  async function handleMinimaxClone(plugin: PluginInfo) {
    const vid = mmCloneVoiceId.trim();
    if (!mmCloneFile || !vid || mmCloning) return;
    const apiKey = settings?.minimax_global_api_key;
    if (!apiKey) {
      setMmCloneMsg({ ok: false, text: "请先在上方填写 MiniMax 国际版 API Key" });
      return;
    }
    setMmCloning(true);
    setMmCloneMsg({ ok: true, text: "正在上传音频并克隆音色，请稍候…" });
    try {
      const clonedId = await minimaxGlobalVoiceClone(
        mmCloneFile,
        vid,
        apiKey,
        MM_GLOBAL_BASE,
        mmShowAdvanced && mmPromptFile ? mmPromptFile : undefined,
        mmShowAdvanced && mmPromptText.trim() ? mmPromptText.trim() : undefined,
      );
      // 持久化克隆音色（首次合成前 get_voice 查不到，本地记录保证下拉可用）
      const cloned = settings?.minimax_global_cloned_voices ?? [];
      if (!cloned.includes(clonedId)) {
        await patch("minimax_global_cloned_voices", [...cloned, clonedId]);
      }
      // 克隆音色无需下载，直接切换
      await patch("plugin_voices", {
        ...(settings?.plugin_voices ?? {}),
        [plugin.id]: clonedId,
      });
      setMmCloneMsg({ ok: true, text: `克隆成功，已切换到音色 ${clonedId}（7 天不使用会被平台回收）` });
      setMmCloneFile("");
      setMmCloneVoiceId("");
      setMmPromptFile("");
      setMmPromptText("");
    } catch (e) {
      setMmCloneMsg({ ok: false, text: String(e) });
    } finally {
      setMmCloning(false);
    }
  }

  /** 刷新 MiniMax 账号音色列表（克隆音色须先合成过一次才会出现） */
  async function handleMmRefreshVoices() {
    const apiKey = settings?.minimax_global_api_key;
    if (!apiKey) {
      setMmManageMsg({ ok: false, text: "请先在上方填写 MiniMax 国际版 API Key" });
      return;
    }
    setMmVoicesLoading(true);
    setMmManageMsg(null);
    try {
      const raw = await minimaxGlobalGetVoices(apiKey, MM_GLOBAL_BASE);
      const j = JSON.parse(raw) as {
        system_voice?: MmAccountVoice[];
        voice_cloning?: MmAccountVoice[];
        voice_generation?: MmAccountVoice[];
      };
      setMmAccountVoices({
        cloning: j.voice_cloning ?? [],
        generation: j.voice_generation ?? [],
        systemCount: (j.system_voice ?? []).length,
      });
    } catch (e) {
      setMmManageMsg({ ok: false, text: String(e) });
    } finally {
      setMmVoicesLoading(false);
    }
  }

  /** 确认删除 MiniMax 账号音色（删除后 voice_id 不可复用） */
  async function confirmMmDelete() {
    if (!mmDeleteTarget) return;
    const { type, id } = mmDeleteTarget;
    setMmDeleteTarget(null);
    const apiKey = settings?.minimax_global_api_key;
    if (!apiKey) {
      setMmManageMsg({ ok: false, text: "请先在上方填写 MiniMax 国际版 API Key" });
      return;
    }
    try {
      await minimaxGlobalDeleteVoice(apiKey, MM_GLOBAL_BASE, type, id);
      // 同步账号列表展示状态
      setMmAccountVoices((v) =>
        v
          ? {
              ...v,
              cloning: type === "voice_cloning" ? v.cloning.filter((x) => x.voice_id !== id) : v.cloning,
              generation: type === "voice_generation" ? v.generation.filter((x) => x.voice_id !== id) : v.generation,
            }
          : v,
      );
      // 同步本地克隆记录
      if (type === "voice_cloning") {
        const cloned = settings?.minimax_global_cloned_voices ?? [];
        if (cloned.includes(id)) {
          await patch("minimax_global_cloned_voices", cloned.filter((x) => x !== id));
        }
      }
      // 若删的是当前选中音色，回退插件默认音色
      if (settings?.plugin_voices?.["minimax-tts-global"] === id) {
        const fallback = plugins.find((p) => p.id === "minimax-tts-global")?.voices[0]?.id ?? "";
        await patch("plugin_voices", {
          ...(settings?.plugin_voices ?? {}),
          "minimax-tts-global": fallback,
        });
      }
      setMmManageMsg({ ok: true, text: `音色 ${id} 已删除（该 ID 不可再复用）` });
    } catch (e) {
      setMmManageMsg({ ok: false, text: String(e) });
    }
  }

  /** 把账号音色设为当前引擎音色 */
  async function handleMmUseVoice(voiceId: string) {
    await patch("plugin_voices", {
      ...(settings?.plugin_voices ?? {}),
      "minimax-tts-global": voiceId,
    });
    setMmManageMsg({ ok: true, text: `已切换到音色 ${voiceId}` });
  }

  const currentVoice = settings?.tts_model ?? "mimo_default";
  const mossVoices = settings?.moss_voices ?? [];

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
    <div className="flex h-full flex-col">
      <div className="scrollbar-thin flex-1 space-y-2.5 overflow-y-auto px-4 py-5 text-sm">
        <div className="mx-auto max-w-xl space-y-2.5">
          {/* 语音合成 */}
          <Section title="语音合成" defaultOpen>
            <Field label="TTS 引擎">
              <select
                value={settings?.tts_engine ?? "mimo"}
                onChange={(e) => handleEngineChange(e.target.value)}
                className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
              >
                <option value="mimo">MiMo（小米）</option>
                <option value="moss">Moss-TTS（Mossland）</option>
                {/* 插件引擎（动态，来自插件管理；排除 ASR 插件） */}
                {plugins
                  .filter((p) => p.loaded && p.plugin_type !== "asr_engine")
                  .map((p) => (
                    <option key={p.id} value={p.id}>{p.name}</option>
                  ))}
              </select>
            </Field>

            {(settings?.tts_engine ?? "mimo") === "mimo" && (
              <>
                <Field label="MiMo API Key">
                  <input
                    type="password"
                    defaultValue={settings?.mimo_api_key ?? ""}
                    onBlur={(e) => onSaveApiKey(e.target.value)}
                    placeholder="sk-..."
                    className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
                  />
                  <a
                    href={`https://platform.xiaomimimo.com?ref=${inviteCode}`}
                    target="_blank"
                    rel="noreferrer"
                    className="mt-1.5 inline-block text-xs text-[var(--ink-500)] underline underline-offset-2 hover:text-[var(--amber-600)] transition-colors"
                  >
                    前往小米 mimo 获取 API
                  </a>
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
                </Field>

                <Field label="默认音色">
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
                </Field>

                <Field label="克隆音色样本">
                  <p className="mb-2.5 text-xs leading-relaxed text-[var(--ink-500)]">
                    导入一段 5–10 秒的本地说话音频（mp3/wav，≤10MB），MiMo 会用它合成相似音色。
                    每次合成都要把样本传给 MiMo，速度比预置音色慢。
                  </p>
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
                </Field>
              </>
            )}

            {settings?.tts_engine === "moss" && (
              <>
                <Field label="Moss-TTS API Key">
                  <input
                    type="password"
                    defaultValue={settings?.moss_api_key ?? ""}
                    onBlur={(e) => patch("moss_api_key", e.target.value.trim())}
                    placeholder="sk-..."
                    className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
                  />
                  <a
                    href="https://platform.mosi.cn/app/api-keys"
                    target="_blank"
                    rel="noreferrer"
                    className="mt-1.5 inline-block text-xs text-[var(--ink-500)] underline underline-offset-2 hover:text-[var(--amber-600)] transition-colors"
                  >
                    前往 Mossland 控制台获取 API Key
                  </a>
                </Field>

                <Field label="当前使用音色">
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
                </Field>

                <Field label="音色库">
                  <div className="space-y-1.5">
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
                </Field>
              </>
            )}

            {/* === 插件引擎配置（音色表来自插件本身，通用） === */}
            {(() => {
              const engineId = settings?.tts_engine ?? "mimo";
              if (engineId === "mimo" || engineId === "moss") return null;
              const cur = plugins.find((p) => p.id === engineId && p.loaded);
              if (!cur) {
                // 当前引擎是插件但不可用：未安装（卸载/换机）或加载失败。
                // 配置块无渲染对象时不能留白，给警示卡片 + 一键切回内置引擎
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
              return (
                <>
                  {cur.id === "edge-tts" && (
                    <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
                      Edge TTS 为<b>免费</b>引擎（微软 Edge 朗读服务），无需 API Key。
                      但它是非官方接口，可能不稳定或受地区限制（部分地区 403），失效不属于软件 bug。
                    </div>
                  )}

                  {/* MiniMax 国内版 API Key */}
                  {cur.id === "minimax-tts" && (
                    <Field label="MiniMax API Key（国内版）">
                      <input
                        type="password"
                        defaultValue={settings?.minimax_api_key ?? ""}
                        onBlur={(e) => patch("minimax_api_key", e.target.value.trim())}
                        placeholder="输入 MiniMax 国内版 API Key"
                        className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
                      />
                      <a
                        href="https://platform.minimaxi.com/user-center/basic-information/interface-key"
                        target="_blank"
                        rel="noreferrer"
                        className="mt-1.5 inline-block text-xs text-[var(--ink-500)] underline underline-offset-2 hover:text-[var(--amber-600)] transition-colors"
                      >
                        前往 MiniMax 平台获取 API Key
                      </a>
                    </Field>
                  )}
                  {/* 国内版阉割版提示：暂不提供音色克隆 */}
                  {cur.id === "minimax-tts" && (
                    <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
                      国内版为<b>阉割版</b>，暂不提供音色克隆。如需克隆自己的音色，请安装并使用 MiniMax 国际版插件。
                    </div>
                  )}

                  {/* MiniMax 国际版 API Key */}
                  {cur.id === "minimax-tts-global" && (
                    <Field label="MiniMax API Key（国际版）">
                      <input
                        type="password"
                        defaultValue={settings?.minimax_global_api_key ?? ""}
                        onBlur={(e) => patch("minimax_global_api_key", e.target.value.trim())}
                        placeholder="输入 MiniMax 国际版 API Key"
                        className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
                      />
                      <a
                        href="https://www.minimax.io"
                        target="_blank"
                        rel="noreferrer"
                        className="mt-1.5 inline-block text-xs text-[var(--ink-500)] underline underline-offset-2 hover:text-[var(--amber-600)] transition-colors"
                      >
                        前往 MiniMax 国际版获取 API Key
                      </a>
                    </Field>
                  )}
                  {/* MiniMax 音色克隆子面板（仅国际版，国内版不提供） */}
                  {cur.id === "minimax-tts-global" && (
                    <div className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5">
                      <p className="text-xs font-medium text-[var(--ink-700)]">音色克隆</p>
                      <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-500)]">
                        上传 10s~5min 的清晰人声音频（mp3/m4a/wav，≤20MB），克隆出的音色 7 天不使用会被平台回收。
                      </p>
                      {/* 音频文件选择（克隆唯一音源，API 必填） */}
                      <div className="mt-2 flex items-center gap-2">
                        <button
                          onClick={async () => {
                            const p = await pickAudioFile();
                            if (p) setMmCloneFile(p);
                          }}
                          className="shrink-0 rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)]"
                        >
                          选择音频
                        </button>
                        <span className="truncate text-[11px] text-[var(--ink-500)]">
                          {mmCloneFile ? mmCloneFile.split(/[/\\]/).pop() : "未选择（必填，克隆的唯一音源）"}
                        </span>
                      </div>
                      {/* 自定义 voice_id */}
                      <input
                        value={mmCloneVoiceId}
                        onChange={(e) => setMmCloneVoiceId(e.target.value)}
                        placeholder="自定义音色 ID（8~256 字符，字母开头）"
                        className="mt-2 w-full rounded-xl border border-[var(--ink-200)] bg-transparent px-3 py-1.5 text-[12px] outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
                      />
                      {/* 高级选项：样本音频 + 对应文字稿（可选，提升克隆相似度） */}
                      <button
                        onClick={() => setMmShowAdvanced((v) => !v)}
                        className="mt-2 text-[11px] text-[var(--ink-500)] underline underline-offset-2 transition-colors hover:text-[var(--amber-600)]"
                      >
                        {mmShowAdvanced ? "收起高级选项 ▲" : "高级选项（可选）▼"}
                      </button>
                      {mmShowAdvanced && (
                        <div className="mt-2 space-y-2 rounded-lg border border-dashed border-[var(--ink-200)] p-2.5">
                          <p className="text-[10px] leading-relaxed text-[var(--ink-500)]">
                            提供一段 8 秒以内的样本音频及其对应文字稿，可提升克隆相似度。
                            注意：本项仅增强效果，不能替代上方必填的主音频（平台接口要求两者同时提供）。
                          </p>
                          <div className="flex items-center gap-2">
                            <button
                              onClick={async () => {
                                const p = await pickAudioFile();
                                if (p) setMmPromptFile(p);
                              }}
                              className="shrink-0 rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)]"
                            >
                              选择样本音频
                            </button>
                            <span className="truncate text-[11px] text-[var(--ink-500)]">
                              {mmPromptFile ? mmPromptFile.split(/[/\\]/).pop() : "未选择"}
                            </span>
                          </div>
                          <input
                            value={mmPromptText}
                            onChange={(e) => setMmPromptText(e.target.value)}
                            placeholder="样本音频对应的文字稿（以标点结尾）"
                            className="w-full rounded-xl border border-[var(--ink-200)] bg-transparent px-3 py-1.5 text-[12px] outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
                          />
                        </div>
                      )}
                      {/* 开始克隆（主音频 + 音色 ID 必填；高级选项仅作增强） */}
                      <button
                        onClick={() => handleMinimaxClone(cur)}
                        disabled={mmCloning || !mmCloneFile || !mmCloneVoiceId.trim()}
                        className="mt-2 rounded-lg bg-[var(--amber-500)] px-3 py-1.5 text-[11px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                      >
                        {mmCloning ? "克隆中…" : "开始克隆"}
                      </button>
                      {!mmCloning && (!mmCloneFile || !mmCloneVoiceId.trim()) && (
                        <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">
                          {!mmCloneFile && !mmCloneVoiceId.trim()
                            ? "请先选择主音频并填写音色 ID"
                            : !mmCloneFile
                              ? "请先选择主音频（10s~5min，高级选项的样本音频不能替代）"
                              : "请填写音色 ID"}
                        </p>
                      )}
                      {mmCloneMsg && (
                        <p className={`mt-1.5 text-[11px] leading-relaxed ${mmCloneMsg.ok ? "text-[var(--ink-500)]" : "text-red-500"}`}>
                          {mmCloneMsg.text}
                        </p>
                      )}
                    </div>
                  )}
                  {/* MiniMax 音色管理面板（仅国际版）：账号音色查询/使用/删除 */}
                  {cur.id === "minimax-tts-global" && (
                    <div className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2.5">
                      <div className="flex items-center justify-between">
                        <p className="text-xs font-medium text-[var(--ink-700)]">音色管理</p>
                        <button
                          onClick={handleMmRefreshVoices}
                          disabled={mmVoicesLoading}
                          className="rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] disabled:opacity-50"
                        >
                          {mmVoicesLoading ? "刷新中…" : "刷新账号音色"}
                        </button>
                      </div>
                      <p className="mt-1 text-[10px] leading-relaxed text-[var(--ink-500)]">
                        克隆音色需先成功合成一次，才会出现在 MiniMax 账号列表中（本地下拉不受影响）。
                      </p>
                      {mmAccountVoices && (
                        <div className="mt-2 space-y-2">
                          {[
                            { title: "克隆音色", type: "voice_cloning", list: mmAccountVoices.cloning },
                            { title: "设计音色", type: "voice_generation", list: mmAccountVoices.generation },
                          ].map((g) => (
                            <div key={g.type}>
                              <p className="text-[10px] font-medium text-[var(--ink-500)]">{g.title}（{g.list.length}）</p>
                              {g.list.length === 0 ? (
                                <p className="mt-1 text-[10px] text-[var(--ink-300)]">暂无</p>
                              ) : (
                                <div className="mt-1 space-y-1">
                                  {g.list.map((v) => (
                                    <div key={v.voice_id} className="flex items-center justify-between rounded-lg border border-[var(--ink-200)] px-2.5 py-1.5">
                                      <div className="min-w-0 flex-1">
                                        <div className="truncate font-mono text-[11px] text-[var(--ink-900)]">{v.voice_id}</div>
                                        {v.created_time && (
                                          <div className="text-[10px] text-[var(--ink-300)]">{v.created_time}</div>
                                        )}
                                      </div>
                                      <div className="ml-2 flex shrink-0 gap-1">
                                        <button
                                          onClick={() => handleMmUseVoice(v.voice_id)}
                                          className="rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)]"
                                        >
                                          使用
                                        </button>
                                        <button
                                          onClick={() => setMmDeleteTarget({ type: g.type, id: v.voice_id })}
                                          className="rounded-md border border-[var(--ink-200)] px-2 py-0.5 text-[10px] text-[var(--seal)] transition-colors hover:border-[var(--seal)]"
                                        >
                                          删除
                                        </button>
                                      </div>
                                    </div>
                                  ))}
                                </div>
                              )}
                            </div>
                          ))}
                          <p className="text-[10px] text-[var(--ink-300)]">系统音色：{mmAccountVoices.systemCount} 个（插件已内置静态列表）</p>
                        </div>
                      )}
                      {/* 行内删除确认（删除后 voice_id 不可复用） */}
                      {mmDeleteTarget && (
                        <div className="mt-2 rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/15 px-3 py-2">
                          <p className="text-[11px] leading-relaxed text-[var(--ink-700)]">
                            确认删除音色「{mmDeleteTarget.id}」？<b className="text-[var(--seal)]">删除后该 ID 不可复用</b>。
                          </p>
                          <div className="mt-1.5 flex gap-2">
                            <button
                              onClick={confirmMmDelete}
                              className="rounded-lg bg-[var(--seal)] px-3 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90"
                            >
                              确认删除
                            </button>
                            <button
                              onClick={() => setMmDeleteTarget(null)}
                              className="rounded-lg border border-[var(--ink-200)] px-3 py-1 text-[11px] text-[var(--ink-500)] hover:border-[var(--ink-300)]"
                            >
                              取消
                            </button>
                          </div>
                        </div>
                      )}
                      {mmManageMsg && (
                        <p className={`mt-1.5 text-[11px] leading-relaxed ${mmManageMsg.ok ? "text-[var(--ink-500)]" : "text-red-500"}`}>
                          {mmManageMsg.text}
                        </p>
                      )}
                    </div>
                  )}
                  <Field label={`${cur.name} 音色`}>
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
                    {/* 切换已装音色时的瞬时预加载指示 */}
                    {preloadingVoice !== null && (
                      <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-[var(--ink-500)]">
                        <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-[var(--amber-500)]" />
                        正在加载音色…
                      </div>
                    )}
                  </Field>

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

                  {/* 音色管理（支持音色安装的本地引擎） */}
                  {cur.has_voice_management && (
                    <VoiceManager
                      plugin={cur}
                      currentVoiceId={settings?.plugin_voices?.[cur.id] ?? cur.voices[0]?.id ?? ""}
                      onChanged={() => {
                        listPlugins().then(setPlugins).catch(() => {});
                      }}
                    />
                  )}
                </>
              );
            })()}
          </Section>

          {/* 虚拟麦克风 */}
          <Section title="虚拟麦克风">
            <MicSettings />
          </Section>

          {/* 语音输入（ASR 插件的配置，仅装了 ASR 插件时显示） */}
          {asrAvailable && (
            <Section title="语音输入">
              <VoiceInputSettings />
            </Section>
          )}

          {/* 快捷键：浮窗呼出 / 语音输入 / 播放最近一条消息 / 开关发送到麦克风 */}
          <Section title="快捷键">
            <div className="space-y-4">
              <HotkeyRow
                label="呼出浮窗"
                value={settings?.hotkey_show_window ?? "Alt+V"}
                onApply={setHotkey}
                hint="按下显示/收起快速输入浮窗。点「录入」后按下想要的组合键（如 Alt+V、Ctrl+Shift+F1）。"
              />
              <HotkeyRow
                label="语音输入（按住说话）"
                value={settings?.voice_input_hotkey ?? ""}
                onApply={setVoiceInputHotkey}
                clearable
                hint="按住快捷键开始录音，松开自动识别并填入输入框（需已安装识别插件）。"
              />
              <HotkeyRow
                label="播放最近一条消息"
                value={settings?.hotkey_play_last ?? ""}
                onApply={setPlayLastHotkey}
                clearable
                hint="按下即播最近一条消息的语音；麦克风发送开关开启时同时发到虚拟麦克风。"
              />
              <HotkeyRow
                label="开关发送到麦克风"
                value={settings?.hotkey_mic_toggle ?? ""}
                onApply={setMicToggleHotkey}
                clearable
                hint="按下切换「语音是否发送到虚拟麦克风」开关，无需鼠标操作。"
              />
            </div>
          </Section>

          {/* 外观 */}
          <Section title="外观">
            <div className="grid grid-cols-2 gap-2">
              {THEMES.map((t) => {
                const active = (settings?.theme ?? "light") === t.id;
                return (
                  <button
                    key={t.id}
                    onClick={() => patch("theme", t.id)}
                    className={[
                      "rounded-xl border px-3 py-2.5 text-left transition-all",
                      active
                        ? "border-[var(--amber-500)] bg-[var(--amber-200)]/30 ring-1 ring-[var(--amber-500)]/40"
                        : "border-[var(--ink-200)] bg-[var(--paper-card)] hover:border-[var(--ink-300)]",
                    ].join(" ")}
                  >
                    <div className={["text-xs font-medium", active ? "text-[var(--amber-600)]" : "text-[var(--ink-700)]"].join(" ")}>
                      {t.label}
                    </div>
                    <div className="mt-0.5 text-[10px] leading-relaxed text-[var(--ink-300)]">
                      {t.desc}
                    </div>
                  </button>
                );
              })}
            </div>
          </Section>

          {/* 关于 */}
          <Section
            title={
              <>
                关于
                {updateDot && (
                  <span className="ml-1.5 inline-block h-1.5 w-1.5 rounded-full bg-[var(--seal)] align-middle" />
                )}
              </>
            }
            defaultOpen={!!updateLatest}
            onOpen={markAboutSeen}
          >
            <div className="text-xs leading-relaxed text-[var(--ink-500)]">
              <div className="font-display text-sm font-medium text-[var(--ink-900)]">电子声带 TTSassist</div>
              <div className="mt-1.5">为语言障碍者打造的文本转语音沟通助手。</div>

              {/* 项目链接 & QQ 群 */}
              <div className="mt-2.5 flex flex-col gap-1.5 text-[11px]">
                <div className="flex items-center gap-2">
                  <span className="text-[var(--ink-400)]">GitHub</span>
                  <button
                    onClick={() => openUrl("https://github.com/Mr-Shaw-Yihan/TTSassist").catch(() => {})}
                    className="text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--amber-700)]"
                  >
                    github.com/Mr-Shaw-Yihan/TTSassist
                  </button>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-[var(--ink-400)]">QQ 群</span>
                  <button
                    onClick={() => {
                      navigator.clipboard.writeText("690907648");
                      const el = document.getElementById("qq-copied-tip");
                      if (el) { el.style.opacity = "1"; setTimeout(() => el.style.opacity = "0", 1500); }
                    }}
                    className="relative font-mono text-[var(--ink-600)] underline decoration-dashed underline-offset-2 hover:text-[var(--amber-600)]"
                    title="点击复制群号"
                  >
                    690907648
                    <span
                      id="qq-copied-tip"
                      className="pointer-events-none absolute -top-6 left-1/2 -translate-x-1/2 rounded bg-[var(--ink-700)] px-1.5 py-0.5 text-[10px] text-[var(--paper)] opacity-0 transition-opacity"
                    >
                      已复制
                    </span>
                  </button>
                </div>
              </div>

              {/* 当前版本 + 检查更新 */}
              <div className="mt-3 flex items-center gap-2">
                <span className="rounded-md bg-[var(--ink-100)] px-2 py-0.5 font-mono text-[11px] text-[var(--ink-500)]">
                  {appVersion ? `v${appVersion}` : "…"}
                </span>
                <button
                  onClick={handleCheckUpdate}
                  disabled={checkingUpdate}
                  className="rounded-lg border border-[var(--ink-200)] px-2.5 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:opacity-50"
                >
                  {checkingUpdate ? "检查中…" : "检查更新"}
                </button>
              </div>

              {/* 检查/启动检查结果 */}
              {updateLatest ? (
                <div className="mt-2.5 rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
                  发现新版本 <span className="font-mono font-medium">v{updateLatest.version}</span>
                  ，建议更新以获得新功能与修复。
                  <button
                    onClick={() => openUrl(updateLatest.url).catch(() => {})}
                    className="ml-1.5 font-medium underline underline-offset-2 hover:text-[var(--ink-700)]"
                  >
                    前往下载
                  </button>
                </div>
              ) : (
                updateChecked && (
                  <div className="mt-2.5 text-[11px] text-[var(--ink-300)]">已是最新版本</div>
                )
              )}

              {/* 免责声明 */}
              <div className="mt-3 rounded-lg border border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2 text-[10px] leading-relaxed text-[var(--ink-300)]">
                <div className="mb-1 font-medium text-[var(--ink-500)]">免责声明</div>
                本软件为开源项目，仅供学习与个人使用。软件仅提供本地服务功能，语音合成能力由第三方运营商服务提供，API Key 由用户自行申请，相关条款与资费以运营商为准。本软件不收集、不上传任何用户个人信息，所有数据仅存储于本地。使用产生的任何后果由用户自行承担。
              </div>
            </div>
          </Section>
        </div>
      </div>
    </div>
  );
}

/** 可折叠的分类（手风琴）。收起时不渲染 children → 减少渲染、消除卡顿。 */
function Section({
  title,
  defaultOpen = false,
  onOpen,
  children,
}: {
  title: React.ReactNode;
  defaultOpen?: boolean;
  /** 展开时回调（如"关于"展开后清红点） */
  onOpen?: () => void;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className="rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)]">
      <button
        onClick={() => {
          const next = !open;
          setOpen(next);
          if (next) onOpen?.();
        }}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-sm font-medium text-[var(--ink-900)]">{title}</span>
        <span className="text-[var(--ink-300)] transition-transform">{open ? "▾" : "▸"}</span>
      </button>
      {open && <div className="space-y-4 px-4 pb-4">{children}</div>}
    </section>
  );
}

/** 分类内的一个设置项（标签 + 内容） */
function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="mb-2 text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">{label}</h3>
      {children}
    </div>
  );
}

/** 快捷键设置行：琥珀竖条 + 衬线标题（与插件页分类标题同款）+ 描述 + 录入器 + 清除按钮 */
function HotkeyRow({
  label,
  value,
  onApply,
  hint,
  clearable = false,
}: {
  label: string;
  value: string;
  onApply: (accel: string) => Promise<void>;
  hint?: string;
  clearable?: boolean;
}) {
  return (
    <div>
      {/* 标题行：与插件页分类头部同款（琥珀竖条 + 衬线标题） */}
      <div className="flex items-center gap-2">
        <span className="h-3.5 w-[3px] shrink-0 rounded-full bg-[var(--amber-500)]" aria-hidden />
        <h3 className="font-display text-sm font-semibold tracking-wide text-[var(--ink-900)]">
          {label}
        </h3>
      </div>
      {hint && (
        <p className="mt-1 pl-[11px] text-[11px] leading-relaxed text-[var(--ink-300)]">{hint}</p>
      )}
      <div className="mt-2 flex items-center gap-2 pl-[11px]">
        <div className="min-w-0 flex-1">
          <HotkeyRecorder value={value} onApply={onApply} />
        </div>
        {clearable && value && (
          <button
            onClick={async () => {
              try {
                await onApply("");
              } catch (e) {
                window.alert(`清除快捷键失败：${e}`);
              }
            }}
            className="shrink-0 rounded-lg border border-[var(--ink-200)] px-2.5 py-2 text-xs text-[var(--ink-300)] transition-colors hover:border-[var(--seal)] hover:text-[var(--seal)]"
          >
            清除
          </button>
        )}
      </div>
    </div>
  );
}