// 设置页面（主界面侧边栏"设置"项的右侧内容区）：分类收纳（手风琴），消除平铺卡顿。
// 分类：语音合成 / 虚拟麦克风 / 快捷键 / 外观 / 关于。
// 虚拟麦克风轮询隔离在 MicSettings 组件，分类收起即停止轮询。

import { useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSettingsStore } from "../../stores/settingsStore";
import { useUpdateStore, shouldShowUpdateDot } from "../../stores/updateStore";
import { importCloneVoice, removeCloneVoice, pickAudioFile, listPlugins } from "../../services/invoke";
import type { MossVoice, PluginInfo } from "../../types";
import { HotkeyRecorder } from "./HotkeyRecorder";
import { MicSettings } from "./MicSettings";

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

  async function copyInvite() {
    try {
      await navigator.clipboard.writeText("U277DH");
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = "U277DH";
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); setCopied(true); setTimeout(() => setCopied(false), 1600); } catch {}
      document.body.removeChild(ta);
    }
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
                onChange={(e) => patch("tts_engine", e.target.value)}
                className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
              >
                <option value="mimo">MiMo（小米）</option>
                <option value="moss">Moss-TTS（Mossland）</option>
                {/* 插件引擎（动态，来自插件管理） */}
                {plugins
                  .filter((p) => p.loaded)
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
                    href="https://platform.xiaomimimo.com?ref=U277DH"
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
                      U277DH
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
              const cur = plugins.find((p) => p.id === settings?.tts_engine && p.loaded);
              if (!cur) return null;
              return (
                <>
                  {cur.id === "edge-tts" && (
                    <div className="rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2 text-[11px] leading-relaxed text-[var(--amber-600)]">
                      Edge TTS 为<b>免费</b>引擎（微软 Edge 朗读服务），无需 API Key。
                      但它是非官方接口，可能不稳定或受地区限制（部分地区 403），失效不属于软件 bug。
                    </div>
                  )}
                  <Field label={`${cur.name} 音色`}>
                    <select
                      value={settings?.plugin_voices?.[cur.id] ?? cur.voices[0]?.id ?? ""}
                      onChange={(e) =>
                        patch("plugin_voices", {
                          ...(settings?.plugin_voices ?? {}),
                          [cur.id]: e.target.value,
                        })
                      }
                      className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
                    >
                      {cur.voices.map((v) => (
                        <option key={v.id} value={v.id}>{v.label}</option>
                      ))}
                    </select>
                  </Field>
                </>
              );
            })()}
          </Section>

          {/* 虚拟麦克风 */}
          <Section title="虚拟麦克风">
            <MicSettings />
          </Section>

          {/* 快捷键 */}
          <Section title="快捷键">
            <HotkeyRecorder />
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