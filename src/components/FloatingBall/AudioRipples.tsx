// 音频波纹：播放期间从球心向外扩散的有机轮廓波纹（非球本体特效层，
// 与粒子/彩带同类）。~30fps；性能模式由父组件门控不渲染。
// 设计：每层波纹从中心诞生 → easeOut 向外扩散 → 抵达画布边缘前淡出；
// 双频正弦扰动出有机形；最大半径含扰动余量，全部内容严格局限于画布内。
// 可视度：每环双色描边（halo）——白色宽描边垫底 + 黑色细描边叠上，
// 黑白灰任何背景下都清晰（地图标注轮廓手法），固定色不随主题反转。

import { useEffect, useRef } from "react";

interface Props {
  /** 画布尺寸（逻辑 px） */
  canvas: number;
  /** 球直径（逻辑 px） */
  ballPx: number;
}

const RINGS = 4;
const SAMPLES = 36;
/** 单层波纹从中心扩散到边缘的周期（秒） */
const CYCLE_S = 2.4;
/** halo 垫底白 / 核心黑：固定色，主题无关 */
const HALO = "#ffffff";
const CORE = "#1a1816";

export function AudioRipples({ canvas, ballPx }: Props) {
  const haloRefs = useRef<(SVGPathElement | null)[]>([]);
  const coreRefs = useRef<(SVGPathElement | null)[]>([]);
  const rafRef = useRef(0);

  useEffect(() => {
    let last = 0;
    const t0 = performance.now();
    const step = (t: number) => {
      rafRef.current = requestAnimationFrame(step);
      if (t - last < 33) return; // ~30fps 足够波纹节奏
      last = t;
      const ts = (t - t0) / 1000;
      const cx = canvas / 2;
      const cy = canvas / 2;
      // 最大半径：画布半径扣除扰动余量（扰动最大约 +11%），保证任何时刻不越出画布
      const rMax = (canvas / 2 - 2) / 1.12;
      const r0 = ballPx * 0.18; // 从球心附近诞生（内段被球体遮住，视觉即「从球后漾出」）
      for (let i = 0; i < RINGS; i++) {
        const halo = haloRefs.current[i];
        const core = coreRefs.current[i];
        if (!halo || !core) continue;
        // 各层错相扩散：p=0 诞生于中心，p=1 抵达边缘
        const p = (ts / CYCLE_S + i / RINGS) % 1;
        const ease = 1 - Math.pow(1 - p, 2); // easeOut：出发快、临边缓
        const r = r0 + (rMax - r0) * ease;
        // 透明度：诞生瞬间快速浮现，扩散过程渐隐，边缘归零（避免越界感与 popping）
        const fade = Math.min(1, p * 5) * (1 - p);
        halo.setAttribute("opacity", (fade * 0.85).toFixed(3));
        core.setAttribute("opacity", (fade * 0.9).toFixed(3));
        const seed = i * 2.7;
        let d = "";
        for (let s = 0; s <= SAMPLES; s++) {
          const a = (s / SAMPLES) * Math.PI * 2;
          // 双频正弦扰动 → 有机轮廓（荡漾感），幅度随半径自然增大但受 rMax 余量约束
          const w =
            Math.sin(a * 3 + ts * 1.8 + seed) * r * 0.07 +
            Math.sin(a * 5 - ts * 2.6 + seed * 1.7) * r * 0.04;
          const rr = r + w;
          const x = cx + Math.cos(a) * rr;
          const y = cy + Math.sin(a) * rr * 0.94;
          d += (s === 0 ? "M" : "L") + x.toFixed(1) + " " + y.toFixed(1);
        }
        d += "Z";
        halo.setAttribute("d", d);
        core.setAttribute("d", d);
      }
    };
    rafRef.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(rafRef.current);
  }, [canvas, ballPx]);

  return (
    <svg className="pointer-events-none absolute inset-0 h-full w-full" aria-hidden>
      {Array.from({ length: RINGS }, (_, i) => (
        <g key={i}>
          {/* halo 垫底：白宽描边，任何深色背景下提供轮廓 */}
          <path
            ref={(el) => {
              haloRefs.current[i] = el;
            }}
            fill="none"
            stroke={HALO}
            strokeWidth={2.6}
            strokeLinejoin="round"
            opacity={0}
          />
          {/* 核心：黑细描边叠上，任何浅色背景下清晰 */}
          <path
            ref={(el) => {
              coreRefs.current[i] = el;
            }}
            fill="none"
            stroke={CORE}
            strokeWidth={1.3}
            strokeLinejoin="round"
            opacity={0}
          />
        </g>
      ))}
    </svg>
  );
}
