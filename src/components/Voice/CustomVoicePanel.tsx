// 云端插件引擎「我的音色」管理区：用户自行保存的音色 ID 列表（如 fish.audio 克隆模型）。
// 与插件内置音色分区呈现：下拉内两个 optgroup（内置/我的），本组件负责「我的」的新增/选用/删除。
// 选中仍走 plugin_voices 通道（与内置音色同一机制），插件把任意音色 id 原样透传，宿主无插件特定逻辑。
import { useState } from "react";
import type { CustomVoiceItem } from "../../types";

interface Props {
  items: CustomVoiceItem[];
  currentVoiceId: string;
  /** 选为当前音色（复用引擎卡现有切换逻辑） */
  onSelect: (id: string) => void;
  onSave: (item: CustomVoiceItem) => void;
  onDelete: (id: string) => void;
}

export function CustomVoicePanel({ items, currentVoiceId, onSelect, onSave, onDelete }: Props) {
  const [label, setLabel] = useState("");
  const [id, setId] = useState("");
  const [error, setError] = useState("");

  const trimmedId = id.trim();
  const trimmedLabel = label.trim();
  const canSave = trimmedId.length > 0 && trimmedLabel.length > 0;

  function handleSave() {
    if (!canSave) return;
    // 音色 ID 必须是可见 ASCII 无空格（fish 为十六进制串、edge 为短名等）；
    // 拦下「把名称填进 ID 框」的反填手滑，避免列表展示与传给插件的值双双出错
    if (!/^[\x21-\x7e]+$/.test(trimmedId)) {
      setError("音色 ID 应为不含空格的非中文标识串（如 afbe646a…），是不是和名称填反了？");
      return;
    }
    if (items.some((x) => x.id === trimmedId)) {
      setError("该音色 ID 已保存过，请直接在上列表选用");
      return;
    }
    onSave({ id: trimmedId, label: trimmedLabel });
    setLabel("");
    setId("");
    setError("");
  }

  return (
    <div className="mt-3 border-t border-[var(--ink-200)] pt-3">
      <p className="text-xs font-medium text-[var(--ink-700)]">我的音色</p>
      <p className="mt-0.5 text-[11px] leading-relaxed text-[var(--ink-500)]">
        从云端服务的音色/模型页获取音色 ID 保存于此（如自己的克隆音色），一次保存，下拉即可选用。
      </p>
      {items.length === 0 ? (
        <p className="mt-2 text-[11px] text-[var(--ink-500)]/70">暂无保存的音色。</p>
      ) : (
        <ul className="mt-2 space-y-1.5">
          {items.map((cv) => (
            <li
              key={cv.id}
              className="flex items-center gap-2 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-2.5 py-1.5"
            >
              <button
                onClick={() => onSelect(cv.id)}
                disabled={currentVoiceId === cv.id}
                className="min-w-0 flex-1 text-left disabled:cursor-default"
                title="选为当前音色"
              >
                <span
                  className={
                    "block truncate text-xs font-medium " +
                    (currentVoiceId === cv.id
                      ? "text-[var(--amber-600)]"
                      : "text-[var(--ink-700)]")
                  }
                >
                  {cv.label}
                  {currentVoiceId === cv.id && " ·使用中"}
                </span>
                <span className="block truncate font-mono text-[10px] text-[var(--ink-500)]">
                  {cv.id}
                </span>
              </button>
              <button
                onClick={() => onDelete(cv.id)}
                className="shrink-0 rounded-md px-1.5 py-1 text-[11px] text-[var(--ink-500)] transition-colors hover:text-[var(--seal)]"
                title="删除该音色"
              >
                删除
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="mt-2 flex items-center gap-1.5">
        <input
          value={label}
          onChange={(e) => {
            setLabel(e.target.value);
            setError("");
          }}
          placeholder="名称（如：我的声音）"
          className="w-2/5 min-w-0 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-2 py-1.5 text-xs outline-none focus:border-[var(--amber-500)]"
        />
        <input
          value={id}
          onChange={(e) => {
            setId(e.target.value);
            setError("");
          }}
          placeholder="音色 ID（粘贴自音色模型页）"
          className="min-w-0 flex-1 rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-2 py-1.5 font-mono text-xs outline-none focus:border-[var(--amber-500)]"
        />
        <button
          onClick={handleSave}
          disabled={!canSave}
          className="shrink-0 rounded-lg bg-[var(--ink-900)] px-3 py-1.5 text-[11px] font-medium text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:opacity-40"
        >
          保存
        </button>
      </div>
      {error && <p className="mt-1 text-[11px] text-[var(--seal)]">{error}</p>}
    </div>
  );
}
