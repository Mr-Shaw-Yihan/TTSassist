// 快捷键录入组件：点「录入」→ 按下组合键 → 自动识别 → 「应用」生效。
// 游戏/Discord 标准的快捷键录入交互。

import { useState, useEffect, useRef } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { setHotkey } from "../../services/invoke";

/** 把键盘事件的特殊键名映射成加速键格式 */
function mapKey(key: string): string {
  if (key.length === 1) return key.toUpperCase(); // 单字母转大写
  switch (key) {
    case " ": return "Space";
    case "ArrowUp": return "Up";
    case "ArrowDown": return "Down";
    case "ArrowLeft": return "Left";
    case "ArrowRight": return "Right";
    case "Escape": return "Escape";
    case "Enter": return "Enter";
    case "Tab": return "Tab";
    case "Backspace": return "Backspace";
    case "Delete": return "Delete";
    default: return key; // F1-F12 等保持原样
  }
}

/** 从键盘事件构建加速键串，如 "Ctrl+Alt+V"。仅按了修饰键时返回 null。 */
function buildAccelerator(e: React.KeyboardEvent): string | null {
  const key = e.key;
  // 仅按下修饰键（还没按主键）→ 继续等待
  if (["Control", "Alt", "Shift", "Meta"].includes(key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");
  parts.push(mapKey(key));
  return parts.join("+");
}

export function HotkeyRecorder() {
  const settings = useSettingsStore((s) => s.settings);
  const current = settings?.hotkey_show_window ?? "Alt+V";

  const [recording, setRecording] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const boxRef = useRef<HTMLDivElement | null>(null);

  // 录入态监听键盘（全局，避免焦点丢失）
  useEffect(() => {
    if (!recording) return;
    function onKey(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
      const accel = buildAccelerator(e as unknown as React.KeyboardEvent);
      if (accel) setPending(accel);
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording]);

  function startRecording() {
    setRecording(true);
    setPending(null);
    setError(null);
  }

  function cancelRecording() {
    setRecording(false);
    setPending(null);
    setError(null);
  }

  async function apply() {
    if (!pending) return;
    setSaving(true);
    setError(null);
    try {
      await setHotkey(pending);
      setRecording(false);
      setPending(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div ref={boxRef}>
      {recording ? (
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <div className="flex-1 rounded-xl border border-[var(--amber-500)] bg-[var(--amber-200)]/20 px-3 py-2 text-center text-sm text-[var(--amber-600)]">
              {pending ? (
                <span className="font-mono font-medium">{pending}</span>
              ) : (
                <span className="animate-pulse">请按下快捷键…</span>
              )}
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={apply}
              disabled={!pending || saving}
              className="flex-1 rounded-lg bg-[var(--ink-900)] px-3 py-1.5 text-xs text-[var(--paper)] transition-colors hover:bg-[var(--ink-700)] disabled:cursor-not-allowed disabled:opacity-40"
            >
              {saving ? "应用中…" : "应用"}
            </button>
            <button
              onClick={cancelRecording}
              className="flex-1 rounded-lg border border-[var(--ink-200)] px-3 py-1.5 text-xs text-[var(--ink-700)] transition-colors hover:bg-[var(--ink-100)]"
            >
              取消
            </button>
          </div>
          {error && (
            <p className="rounded-lg border border-[var(--seal)]/30 bg-[var(--seal)]/10 px-3 py-1.5 text-[11px] text-[var(--seal)]">
              ✗ {error}
            </p>
          )}
        </div>
      ) : (
        <div className="flex items-center gap-2">
          <div className="flex-1 rounded-xl border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-center font-mono text-sm text-[var(--ink-900)]">
            {current}
          </div>
          <button
            onClick={startRecording}
            className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-xs text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
          >
            录入
          </button>
        </div>
      )}
      {!recording && (
        <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">
          点「录入」后按下想要的组合键（如 Alt+V、Ctrl+Shift+F1）。
        </p>
      )}
    </div>
  );
}