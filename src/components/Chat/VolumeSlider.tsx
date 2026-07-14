// 悬浮音量控件：默认一个小球，hover 展开音量+播放速度滑块卡片，
// 长按 300ms 进入拖拽模式（卡片收回），松开停在新位置。位置限定在窗口内。
// 大纲 4.6.2

import { useRef, useState, useEffect } from "react";
import { useSettingsStore } from "../../stores/settingsStore";

const BALL_SIZE = 36;          // 球直径 px
const DRAG_DELAY_MS = 300;     // 长按超过此时长判定为拖拽
const EDGE = 4;                // 离窗口边最小距离

export function FloatingBall() {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const volume = settings?.playback_volume ?? 0.8;
  const rate = settings?.playback_rate ?? 1.0;

  // 球位置（左上角坐标）。默认顶部居中。
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  // 是否展开滑块卡片
  const [expanded, setExpanded] = useState(false);
  // 是否处于拖拽中
  const draggingRef = useRef(false);
  const pressTimerRef = useRef<number | null>(null);
  const dragOffsetRef = useRef<{ dx: number; dy: number }>({ dx: 0, dy: 0 });
  const ballRef = useRef<HTMLDivElement | null>(null);

  // 默认顶部居中：首次布局时根据窗口宽度算
  useEffect(() => {
    if (pos === null) {
      const w = window.innerWidth;
      setPos({ x: Math.max(EDGE, Math.floor(w / 2 - BALL_SIZE / 2)), y: EDGE });
    }
  }, [pos]);

  // 拖拽时全局监听 mousemove/mouseup
  useEffect(() => {
    if (!draggingRef.current) return;
    function onMove(e: MouseEvent) {
      if (!draggingRef.current) return;
      const w = window.innerWidth;
      const h = window.innerHeight;
      const x = Math.min(w - BALL_SIZE - EDGE, Math.max(EDGE, e.clientX - dragOffsetRef.current.dx));
      const y = Math.min(h - BALL_SIZE - EDGE, Math.max(EDGE, e.clientY - dragOffsetRef.current.dy));
      setPos({ x, y });
    }
    function onUp() {
      draggingRef.current = false;
      if (pressTimerRef.current) { clearTimeout(pressTimerRef.current); pressTimerRef.current = null; }
      setExpanded(false); // 松开后不论 hover 与否先收回，等下次 mouseenter 触发展开
      document.body.style.userSelect = "";
    }
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    document.body.style.userSelect = "none";
    return () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.userSelect = "";
    };
  }, [draggingRef.current]);

  function onMouseDown(e: React.MouseEvent) {
    // 仅响应主鼠标键
    if (e.button !== 0) return;
    const rect = (e.currentTarget as HTMLDivElement).getBoundingClientRect();
    dragOffsetRef.current = { dx: e.clientX - rect.left, dy: e.clientY - rect.top };
    // 300ms 后进入拖拽
    pressTimerRef.current = window.setTimeout(() => {
      draggingRef.current = true;
      setExpanded(false); // 长按时收回卡片
    }, DRAG_DELAY_MS);
  }

  function onMouseUp() {
    if (pressTimerRef.current) { clearTimeout(pressTimerRef.current); pressTimerRef.current = null; }
    // 短按不触发拖拽，此时由 mouseenter/mouseleave 控制 expanded
  }

  function onMouseEnter() {
    // 拖拽中不处理
    if (draggingRef.current) return;
    setExpanded(true);
  }

  function onMouseLeave() {
    if (draggingRef.current) return;
    setExpanded(false);
  }

  if (pos === null) return null;

  return (
    <div
      className="absolute z-30"
      style={{ left: pos.x, top: pos.y, width: BALL_SIZE, height: BALL_SIZE }}
    >
      {/* 卡片：球右侧展开两个滑块 */}
      {expanded && (
        <div
          className="absolute left-[44px] top-0 flex flex-col gap-2 rounded-xl border border-gray-200 bg-white p-3 shadow-lg"
          onMouseEnter={() => setExpanded(true)}
          onMouseLeave={() => setExpanded(false)}
        >
          <Slider
            icon="🔊"
            value={volume}
            min={0}
            max={1}
            step={0.05}
            format={(v) => `${Math.round(v * 100)}%`}
            onChange={(v) => patch("playback_volume", v)}
          />
          <Slider
            icon="⚡"
            value={rate}
            min={0.5}
            max={2}
            step={0.1}
            format={(v) => `${v.toFixed(1)}x`}
            onChange={(v) => patch("playback_rate", v)}
          />
        </div>
      )}

      {/* 球本身 */}
      <div
        ref={ballRef}
        onMouseDown={onMouseDown}
        onMouseUp={onMouseUp}
        onMouseEnter={onMouseEnter}
        onMouseLeave={onMouseLeave}
        className={[
          "flex cursor-grab items-center justify-center rounded-full",
          "bg-blue-500/85 text-white shadow-md",
          draggingRef.current ? "cursor-grabbing scale-110" : "",
          "transition-transform",
        ].join(" ")}
        style={{ width: BALL_SIZE, height: BALL_SIZE }}
        title="鼠标移上去展开调节；长按可拖动"
      >
        <span className="text-base select-none">🔊</span>
      </div>
    </div>
  );
}

function Slider({
  icon,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  icon: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="text-sm">{icon}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="w-24 accent-blue-500"
      />
      <span className="w-10 text-xs tabular-nums text-gray-500">{format(value)}</span>
    </div>
  );
}