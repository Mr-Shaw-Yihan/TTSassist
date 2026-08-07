// 实时音量条：跟随录音器的音量（0~1）绘制填充条。
// 用 requestAnimationFrame 直改 DOM 宽度（不走 setState），60fps 也不触发 React 重渲染；
// 每帧做平滑回落，声音间断时条形缓慢衰减而非瞬间归零。

import { useEffect, useRef } from "react";
import type { AudioRecorder } from "../../utils/audioRecorder";

interface Props {
  /** 正在录音的录音器；组件应在录音结束后卸载 */
  recorder: AudioRecorder;
  /** 外层容器（决定条的尺寸/配色环境） */
  className?: string;
  /** 填充条配色，默认主题琥珀色 */
  barClassName?: string;
}

export function VolumeMeter({
  recorder,
  className = "",
  barClassName = "bg-[var(--amber-500)]",
}: Props) {
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let raf = 0;
    let level = 0;
    const tick = () => {
      const target = recorder.volume;
      // 上升快（跟随说话）、下降慢（平滑回落）
      level = target > level ? target : level * 0.92;
      if (barRef.current) {
        barRef.current.style.width = `${Math.round(level * 100)}%`;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [recorder]);

  return (
    <div
      className={`overflow-hidden rounded-full bg-[var(--ink-100)] ${className}`}
      aria-label="麦克风音量"
    >
      <div ref={barRef} className={`h-full rounded-full transition-none ${barClassName}`} style={{ width: 0 }} />
    </div>
  );
}
