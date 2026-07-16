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

interface Props {
  onClose: () => void;
}

export function SettingsDrawer({ onClose }: Props) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [importing, setImporting] = useState(false);
  const [cloneName, setCloneName] = useState(settings?.clone_voice_name ?? "");

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
    <div className="fixed inset-0 z-[60] flex">
      {/* 遮罩 */}
      <div className="flex-1 bg-black/30" onClick={onClose} />
      {/* 抽屉 */}
      <div className="flex h-full w-80 flex-col bg-white shadow-2xl">
        {/* 头 */}
        <div className="flex items-center justify-between border-b px-4 py-3">
          <span className="text-sm font-semibold">设置</span>
          <button
            onClick={onClose}
            className="rounded px-2 py-1 text-gray-500 hover:bg-gray-100"
          >
            ✕
          </button>
        </div>

        <div className="scrollbar-thin flex-1 space-y-5 overflow-y-auto px-4 py-4 text-sm">
          {/* API Key */}
          <Section title="MiMo API Key">
            <input
              type="text"
              defaultValue={settings?.mimo_api_key ?? ""}
              onBlur={(e) => onSaveApiKey(e.target.value)}
              placeholder="sk-..."
              className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm outline-none focus:border-blue-400"
            />
            <a
              href="https://platform.xiaomimimo.com"
              target="_blank"
              rel="noreferrer"
              className="mt-1 inline-block text-xs text-blue-500 hover:underline"
            >
              前往 platform.xiaomimimo.com 注册/领取
            </a>
          </Section>

          {/* 音色选择 */}
          <Section title="默认音色">
            <select
              value={currentVoice}
              onChange={(e) => onPickVoice(e.target.value)}
              className="w-full rounded-lg border border-gray-200 px-3 py-2 text-sm outline-none focus:border-blue-400"
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
            <p className="mb-2 text-xs leading-relaxed text-gray-500">
              导入一段 5-10 秒的本地说话音频（mp3/wav，≤10MB），MiMo 会用它合成相似音色。
              每次合成都要把样本传给 MiMo，速度比预置音色慢。
            </p>
            {settings?.clone_voice_path ? (
              <div className="space-y-2">
                <div className="rounded-lg bg-gray-50 px-3 py-2 text-xs">
                  当前样本：<span className="font-medium">{settings.clone_voice_name || "未命名"}</span>
                </div>
                <button
                  onClick={onImportClone}
                  disabled={importing}
                  className="w-full rounded-lg border border-gray-200 px-3 py-1.5 text-xs hover:bg-gray-50 disabled:opacity-50"
                >
                  {importing ? "导入中…" : "替换样本"}
                </button>
                <button
                  onClick={onRemoveClone}
                  className="w-full rounded-lg px-3 py-1.5 text-xs text-red-500 hover:bg-red-50"
                >
                  删除克隆样本
                </button>
              </div>
            ) : (
              <button
                onClick={onImportClone}
                disabled={importing}
                className="w-full rounded-lg bg-blue-500 px-3 py-2 text-xs text-white hover:bg-blue-600 disabled:opacity-50"
              >
                {importing ? "导入中…" : "+ 导入样本"}
              </button>
            )}
          </Section>

          {/* 快捷键（字段先做，功能下一阶段） */}
          <Section title="呼出浮窗快捷键">
            <input
              type="text"
              defaultValue={settings?.hotkey_show_window ?? "Alt+V"}
              onBlur={(e) => patch("hotkey_show_window", e.target.value.trim())}
              disabled
              className="w-full rounded-lg border border-gray-200 bg-gray-50 px-3 py-2 text-sm text-gray-500"
            />
            <p className="mt-1 text-xs text-gray-400">快捷键功能开发中，暂不可用</p>
          </Section>

          {/* 关于 */}
          <Section title="关于">
            <div className="text-xs leading-relaxed text-gray-600">
              <div className="font-medium text-gray-800">VoiceAssist</div>
              <div className="mt-1">为语言障碍者打造的文本转语音沟通助手。</div>
              <div className="mt-2">TTS 引擎：小米 MiMo v2.5</div>
              <a
                href="https://mimo.mi.com/docs/zh-CN/quick-start/usage-guide/audio/speech-synthesis-v2.5"
                target="_blank"
                rel="noreferrer"
                className="mt-1 inline-block text-blue-500 hover:underline"
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
      <h3 className="mb-2 text-xs font-semibold uppercase text-gray-500">{title}</h3>
      {children}
    </section>
  );
}