// 克隆音色样本卡：显示与否由插件自述（`voices` 里含 id="clone" 的项就渲染）。
// 前端不认识任何引擎：支持哪些格式、大小上限、克隆怎么生效，全由插件判定；
// 音色名里的提醒（如「暂不支持低延迟」）也来自插件的 voices.label，前端只渲染不改写。

import { useState } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { getSettings, importCloneVoice, pickAudioFile, removeCloneVoice } from "../../services/invoke";
import { SubPanel } from "../common/SettingsSection";
import { toast } from "../common/Toast";

export function CloneSamplePanel({
  selected,
  onImported,
  onRemoved,
}: {
  /** 当前是否已选用克隆音色 */
  selected: boolean;
  /** 导入成功后把当前音色切到克隆（切法由父组件按通用机制决定） */
  onImported: () => void | Promise<void>;
  /** 删除样本后把不再可用的克隆音色退回去（退回哪里由父组件按插件 voices 决定） */
  onRemoved: () => void | Promise<void>;
}) {
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [importing, setImporting] = useState(false);

  async function onImport() {
    const filePath = await pickAudioFile();
    if (!filePath) return;
    const name = window.prompt(
      "给这个克隆音色起个名字：",
      settings?.clone_voice_name || "我的音色",
    );
    if (!name || !name.trim()) return;
    setImporting(true);
    try {
      await importCloneVoice(filePath, name.trim());
      // 顺序不能反：先等父组件把音色切到 clone（patch 自带后端回读），
      // 再重拉一次兼并样本字段。先重拉会把刚写下的音色选择刷掉（两写者抢）
      await onImported();
      setSettings(await getSettings());
    } catch (e) {
      toast(`导入克隆样本失败：${e}`, "err");
    } finally {
      setImporting(false);
    }
  }

  async function onRemove() {
    if (!window.confirm("确认删除克隆音色样本？")) return;
    try {
      await removeCloneVoice();
      // 同导入：先让父组件把选中音色退回（可能触发一次 patch），再重拉作为终态
      await onRemoved();
      setSettings(await getSettings());
    } catch (e) {
      toast(`删除失败：${e}`, "err");
    }
  }

  return (
    <SubPanel
      title="音色克隆"
      desc="导入一段 5–10 秒的本地说话音频作为克隆样本。支持的格式与大小上限由当前引擎自行判定；用克隆音色合成时需随请求上传样本，首响比预置音色慢。"
    >
      {settings?.clone_voice_path ? (
        <div className="space-y-2">
          <div className="rounded-xl border border-[var(--ink-200)] bg-[var(--ink-100)]/50 px-3 py-2 text-xs text-[var(--ink-500)]">
            当前样本：<span className="font-medium text-[var(--ink-900)]">{settings.clone_voice_name || "未命名"}</span>
            {selected && (
              <span className="ml-2 text-[var(--amber-600)]">（已选用）</span>
            )}
          </div>
          <button
            onClick={onImport}
            disabled={importing}
            className="w-full rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-xs text-[var(--ink-700)] hover:border-[var(--amber-500)] hover:text-[var(--amber-600)] transition-colors disabled:opacity-50"
          >
            {importing ? "导入中…" : "替换样本"}
          </button>
          <button
            onClick={onRemove}
            className="w-full rounded-xl px-3 py-2 text-xs text-[var(--seal)] hover:bg-[var(--seal)]/10 transition-colors"
          >
            删除克隆样本
          </button>
        </div>
      ) : (
        <button
          onClick={onImport}
          disabled={importing}
          className="w-full rounded-xl bg-[var(--ink-900)] px-3 py-2 text-xs text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:opacity-40"
        >
          {importing ? "导入中…" : "+ 导入样本"}
        </button>
      )}
    </SubPanel>
  );
}
