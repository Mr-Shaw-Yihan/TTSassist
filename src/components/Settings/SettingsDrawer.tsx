// 设置面板（右侧抽屉）：API Key / 音色 / 克隆音色 / 快捷键 / 关于
// 大纲 9.1-9.4。从右侧滑入，点遮罩或关闭按钮收回。

import { useState, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { importCloneVoice, removeCloneVoice, pickAudioFile, listMicDevices, checkVbCable, testMic, getMicStatus } from "../../services/invoke";
import type { MossVoice, AudioDevice, MicStatus } from "../../types";
import { HotkeyRecorder } from "./HotkeyRecorder";

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

interface Props {
  onClose: () => void;
}

export function SettingsDrawer({ onClose }: Props) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [importing, setImporting] = useState(false);
  const [cloneName, setCloneName] = useState(settings?.clone_voice_name ?? "");
  const [copied, setCopied] = useState(false);

  // 音色库管理状态
  const [addName, setAddName] = useState("");
  const [addId, setAddId] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editId, setEditId] = useState("");

  // 虚拟麦克风设备
  const [micDevices, setMicDevices] = useState<AudioDevice[]>([]);
  const [vbInstalled, setVbInstalled] = useState(true);
  const [micStatus, setMicStatus] = useState<MicStatus | null>(null);
  const [testing, setTesting] = useState(false);
  useEffect(() => {
    listMicDevices()
      .then((d) => setMicDevices(d))
      .catch(() => {});
    checkVbCable()
      .then((v) => setVbInstalled(v))
      .catch(() => {});
  }, []);

  // 实时轮询麦克风状态（每 600ms），让发送消息后能立刻看到麦克风线程的动作
  useEffect(() => {
    let alive = true;
    const timer = setInterval(async () => {
      if (!alive) return;
      try {
        setMicStatus(await getMicStatus());
      } catch { /* ignore */ }
    }, 600);
    return () => { alive = false; clearInterval(timer); };
  }, []);

  // 播放测试音（结果由上面的轮询自动显示）
  async function runMicTest() {
    const device = settings?.mic_output_device ?? "";
    if (!device) return;
    setTesting(true);
    try {
      await testMic(device, settings?.mic_playback_volume ?? 1.0);
      setTimeout(() => setTesting(false), 400);
    } catch (e) {
      setMicStatus({ is_playing: false, current_device: device, volume: 1, last_error: String(e), last_source: null });
      setTesting(false);
    }
  }

  async function copyInvite() {
    try {
      await navigator.clipboard.writeText("U277DH");
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      // 兜底：用 execCommand
      const ta = document.createElement("textarea");
      ta.value = "U277DH";
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); setCopied(true); setTimeout(() => setCopied(false), 1600); } catch {}
      document.body.removeChild(ta);
    }
  }

  // 当前音色值：若已克隆则为特殊值 "clone"
  const currentVoice = settings?.tts_model ?? "mimo_default";

  // ── 音色库管理 ──
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
    // 删的是当前音色 → 切到剩余第一个
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
    // 编辑的是当前选中音色且 id 变了 → 更新 moss_voice_id
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
      // 选中克隆音色：把 tts_model 设为 "clone"
      await patch("tts_model", "clone");
      // 重新拉一次 settings（克隆命令在后端改了 settings，事件应已触发；这里兜底）
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
    <div className="fixed inset-0 z-[60] flex animate-fade">
      {/* 遮罩 ── 墨色薄纱 */}
      <div className="flex-1 bg-[var(--ink-900)]/25" onClick={onClose} />
      {/* 抽屉 ── 从右滑入的纸面 */}
      <div className="flex h-full w-80 flex-col bg-[var(--paper)] shadow-[-12px_0_32px_rgba(26,24,22,0.10)] animate-drawer">
        {/* 头 ── 品牌字标题 */}
        <div className="flex items-center justify-between border-b border-[var(--ink-200)] px-5 py-4">
          <span className="font-display text-base text-[var(--ink-900)]">设置</span>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-[var(--ink-300)] transition-colors hover:bg-[var(--ink-100)] hover:text-[var(--ink-700)]"
          >
            ×
          </button>
        </div>

        <div className="scrollbar-thin flex-1 space-y-6 overflow-y-auto px-5 py-5 text-sm">
          {/* 皮肤 */}
          <Section title="皮肤">
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

          {/* 虚拟麦克风 */}
          <Section title="虚拟麦克风">
            {!vbInstalled ? (
              <div className="mb-2 rounded-lg border border-[var(--amber-200)] bg-[var(--amber-200)]/20 px-3 py-2.5 text-[11px] leading-relaxed text-[var(--amber-600)]">
                <p className="font-medium">未检测到 VB-CABLE 虚拟声卡</p>
                <p className="mt-1">要让队友听到语音，需先安装 VB-CABLE 驱动：</p>
                <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
                  <a
                    href="https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v1.1.0/VBCABLE_Driver_Pack45.zip"
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 rounded-md bg-[var(--amber-500)] px-2 py-1 font-medium text-[var(--paper)] no-underline transition-colors hover:bg-[var(--amber-600)]"
                  >
                    ⬇ 下载驱动包
                  </a>
                  <a href="https://vb-audio.com/Cable/" target="_blank" rel="noreferrer"
                     className="underline underline-offset-2 hover:text-[var(--seal)] transition-colors">
                    或访问官网
                  </a>
                </div>
                <p className="mt-1.5 text-[10px] text-[var(--amber-600)]/80">
                  解压后右键以管理员身份运行 VBCABLE_Setup_x64.exe，装完重启电脑。
                </p>
                <p className="mt-1 text-[10px] text-[var(--ink-300)]">
                  VB-CABLE 是捐赠软件（donationware），来源 www.vb-cable.com，欢迎向作者捐赠。
                </p>
              </div>
            ) : (
              <p className="mb-2 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-[11px] text-[var(--ink-500)]">
                ✓ 已检测到 VB-CABLE 虚拟声卡
              </p>
            )}
            <label className="mb-1 block text-[11px] text-[var(--ink-300)]">输出设备（选 CABLE Input）</label>
            <select
              value={settings?.mic_output_device ?? ""}
              onChange={(e) => patch("mic_output_device", e.target.value)}
              className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
            >
              <option value="">未配置</option>
              {[...micDevices]
                .sort((a, b) => Number(b.is_virtual_cable) - Number(a.is_virtual_cable))
                .map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}{d.is_virtual_cable ? "（虚拟声卡）" : ""}{d.is_default ? "（默认）" : ""}
                  </option>
                ))}
            </select>
            <div className="mt-2.5 flex items-center gap-2">
              <span className="text-[11px] text-[var(--ink-300)]">麦克风音量</span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={settings?.mic_playback_volume ?? 1.0}
                onChange={(e) => patch("mic_playback_volume", parseFloat(e.target.value))}
                className="flex-1 accent-[var(--amber-600)]"
              />
              <span className="w-8 text-[11px] tabular-nums text-[var(--ink-500)]">
                {Math.round((settings?.mic_playback_volume ?? 1.0) * 100)}%
              </span>
            </div>
            <div className="mt-2.5 flex items-center gap-2">
              <button
                onClick={runMicTest}
                disabled={testing || !(settings?.mic_output_device)}
                className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-1.5 text-xs text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] disabled:cursor-not-allowed disabled:opacity-40"
              >
                {testing ? "测试中…" : "🔊 测试麦克风"}
              </button>
              <span className="text-[11px] text-[var(--ink-300)]">播放 1.2 秒测试音到所选设备</span>
            </div>
            {micStatus && (
              <div className="mt-1.5 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-[11px] leading-relaxed">
                {micStatus.last_error ? (
                  <span className="text-[var(--seal)]">✗ {micStatus.last_error}</span>
                ) : micStatus.is_playing ? (
                  <span className="text-[var(--amber-600)]">
                    🔊 正在播放到「{micStatus.current_device}」…（{micStatus.last_source}）
                  </span>
                ) : micStatus.last_source ? (
                  <span className="text-[var(--ink-500)]">
                    ✓ 上次已发送到「{micStatus.current_device}」（{micStatus.last_source}）
                  </span>
                ) : (
                  <span className="text-[var(--ink-300)]">空闲。点「测试麦克风」或发一条消息试试。</span>
                )}
                {micStatus.last_source && !micStatus.last_error && (
                  <div className="mt-1 text-[var(--ink-300)]">
                    若此处显示已发送但对方听不到：请确认通话软件麦克风设为「CABLE Output」。
                  </div>
                )}
              </div>
            )}
            <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">
              开启工具栏🎙️开关后，发送的语音会发到所选设备。请在游戏/通话软件里把麦克风设为「CABLE Output」。
            </p>
          </Section>

          {/* TTS 引擎选择 */}
          <Section title="TTS 引擎">
            <select
              value={settings?.tts_engine ?? "mimo"}
              onChange={(e) => patch("tts_engine", e.target.value)}
              className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none focus:border-[var(--amber-500)]"
            >
              <option value="mimo">MiMo（小米）</option>
              <option value="moss">Moss-TTS（Mossland）</option>
            </select>
          </Section>

          {/* === MiMo 引擎配置 === */}
          {(settings?.tts_engine ?? "mimo") === "mimo" && (
          <>
          {/* API Key */}
          <Section title="MiMo API Key">
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
          </Section>

          {/* 音色选择 */}
          <Section title="默认音色">
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
          </Section>

          {/* 克隆音色 */}
          <Section title="克隆音色样本">
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
          </Section>
          </>
          )}

          {/* === Moss-TTS 引擎配置 === */}
          {settings?.tts_engine === "moss" && (
          <>
            <Section title="Moss-TTS API Key">
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
            </Section>

            <Section title="当前使用音色">
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
            </Section>

            <Section title="音色库">
              <div className="space-y-1.5">
                {mossVoices.map((v) => (
                  editingId === v.voice_id ? (
                    // 行内编辑态
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
                    // 展示态
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

              {/* 添加表单 */}
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
            </Section>
          </>
          )}

          {/* 呼出浮窗快捷键（按键录入自定义） */}
          <Section title="呼出浮窗快捷键">
            <HotkeyRecorder />
          </Section>

          {/* 关于 */}
          <Section title="关于">
            <div className="text-xs leading-relaxed text-[var(--ink-500)]">
              <div className="font-display text-sm font-medium text-[var(--ink-900)]">语笺 VoiceAssist</div>
              <div className="mt-1.5">为语言障碍者打造的文本转语音沟通助手。</div>
              <div className="mt-2 text-[var(--ink-300)]">TTS 引擎 · 小米 MiMo v2.5</div>
              <a
                href="https://mimo.mi.com/docs/zh-CN/quick-start/usage-guide/audio/speech-synthesis-v2.5"
                target="_blank"
                rel="noreferrer"
                className="mt-1.5 inline-block text-[var(--amber-600)] underline underline-offset-2 hover:text-[var(--ink-700)] transition-colors"
              >
                MiMo TTS 文档
              </a>
            </div>
          </Section>
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h3 className="mb-2.5 text-[10px] font-medium uppercase tracking-[0.25em] text-[var(--ink-300)]">{title}</h3>
      {children}
    </section>
  );
}