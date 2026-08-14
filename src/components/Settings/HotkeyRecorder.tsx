// 快捷键录入组件（通用）：点「录入」→ 按下组合键 → 自动识别 → 「应用」生效。
// 游戏/Discord 标准的快捷键录入交互。
// 泛化后供浮窗快捷键与语音输入快捷键共用：value 提供当前值，onApply 负责后端注册与持久化。
// 互斥录制：全局同一时间只允许一个录入器处于录制态（模块级注册表 + 订阅通知）。

import { useState, useEffect, useRef } from "react";
import { useSettingsStore } from "../../stores/settingsStore";
import { getSettings } from "../../services/invoke";
import { buildAccelerator } from "../../utils/accelerator";

// ── 录制互斥注册表（模块级）：新录入器开始录制时，其它正在录制的自动退出 ──
let activeRecorderId: number | null = null;
const recorderListeners = new Set<(id: number | null) => void>();

function claimRecorder(id: number) {
  activeRecorderId = id;
  recorderListeners.forEach((fn) => fn(id));
}

/** 释放录制权（幂等：仅当自己是当前持有者时才通知） */
function releaseRecorder(id: number) {
  if (activeRecorderId !== id) return;
  activeRecorderId = null;
  recorderListeners.forEach((fn) => fn(null));
}

interface Props {
  /** 当前已设置的快捷键（空串显示「未设置」） */
  value: string;
  /** 应用新快捷键（后端验证+注册+持久化） */
  onApply: (accel: string) => Promise<void>;
  /** 录入提示文案 */
  hint?: string;
}

export function HotkeyRecorder({ value, onApply, hint }: Props) {
  const setSettings = useSettingsStore((s) => s.setSettings);
  const current = value;

  const [recording, setRecording] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const boxRef = useRef<HTMLDivElement | null>(null);
  // 实例唯一 id（用于录制互斥注册表）
  const idRef = useRef(Math.floor(Math.random() * 1e9));

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

  // 录制互斥：别的录入器开始录制时，自己退出录制态
  useEffect(() => {
    if (!recording) return;
    const fn = (activeId: number | null) => {
      if (activeId !== idRef.current) {
        setRecording(false);
        setPending(null);
        setError(null);
      }
    };
    recorderListeners.add(fn);
    return () => {
      recorderListeners.delete(fn);
      releaseRecorder(idRef.current);
    };
  }, [recording]);

  function startRecording() {
    setRecording(true);
    setPending(null);
    setError(null);
    // 异步认领：等本组件的订阅 effect 先挂载，再通知其它录入器退出，
    // 避免同步通知时自己刚加的监听器被误触发（activeId 判断已兜底，此处双保险）
    const id = idRef.current;
    queueMicrotask(() => claimRecorder(id));
  }

  function cancelRecording() {
    releaseRecorder(idRef.current);
    setRecording(false);
    setPending(null);
    setError(null);
  }

  async function apply() {
    if (!pending) return;
    setSaving(true);
    setError(null);
    try {
      await onApply(pending);
      // 立即刷新 store，让显示即时更新（不等事件异步往返）
      setSettings(await getSettings());
      setRecording(false);
      setPending(null);
    } catch (e) {
      setError(String(e));
    } finally {
      releaseRecorder(idRef.current);
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
            {current || <span className="text-[var(--ink-300)]">未设置</span>}
          </div>
          <button
            onClick={startRecording}
            className="rounded-lg border border-[var(--ink-200)] bg-[var(--paper-card)] px-3 py-2 text-xs text-[var(--ink-700)] transition-colors hover:border-[var(--amber-500)] hover:text-[var(--amber-600)]"
          >
            录入
          </button>
        </div>
      )}
      {!recording && hint && (
        <p className="mt-1.5 text-[11px] leading-relaxed text-[var(--ink-300)]">{hint}</p>
      )}
    </div>
  );
}