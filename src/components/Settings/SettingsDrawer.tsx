// 设置面板（右侧抽屉）：API Key / 音色 / 克隆音色 / 快捷键 / 关于
// 大纲 9.1-9.4。从右侧滑入，点遮罩或关闭按钮收回。

import { useState } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { importCloneVoice, removeCloneVoice, pickAudioFile } from "../../services/invoke";

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

            <Section title="Moss-TTS 音色 ID">
              <input
                type="text"
                defaultValue={settings?.moss_voice_id ?? ""}
                onBlur={(e) => patch("moss_voice_id", e.target.value.trim())}
                placeholder="voice_id..."
                className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[var(--ink-300)] focus:border-[var(--amber-500)]"
              />
              <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">
                在 Mossland 控制台「音色」页面创建音色后，复制 voice_id 填入。
              </p>
            </Section>
          </>
          )}

          {/* 快捷键（字段先做，功能下一阶段） */}
          <Section title="呼出浮窗快捷键">
            <input
              type="text"
              defaultValue={settings?.hotkey_show_window ?? "Alt+V"}
              onBlur={(e) => patch("hotkey_show_window", e.target.value.trim())}
              disabled
              className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--ink-100)]/40 px-3 py-2 text-sm text-[var(--ink-300)]"
            />
            <p className="mt-1.5 text-xs text-[var(--ink-300)]">快捷键功能开发中，暂不可用</p>
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