// 输入区（composer）高度拖拽的纯计算逻辑，与 React/DOM 解耦，便于单测。
// 分隔条在 App.tsx 中拖拽调整输入区高度（向上拖增大、消息区随之缩小）。
// 关键：给定「按下时的起始高度 + 按下时的锚点 y + 当前指针 y」，用绝对位移线性求值，
// 不做逐帧累加——累加会把全程位移反复叠加，导致高度加速暴涨、一拖就撞边界。

/** 输入区默认高度（px） */
export const COMPOSER_DEF = 172;
/** 输入区最小高度（px）：再小会挤掉工具栏与多行输入 */
export const COMPOSER_MIN = 118;
/** 输入区最大高度占视口比例：超过它消息区就没看头了 */
export const COMPOSER_MAX_RATIO = 0.55;

export interface ComposerHeightArgs {
  /** 拖拽开始时的输入区高度（px），全程固定 */
  startH: number;
  /** 拖拽开始时指针的 clientY（px），全程固定，作为位移锚点 */
  anchorY: number;
  /** 当前指针 clientY（px） */
  clientY: number;
  /** 最小高度（px） */
  min: number;
  /** 最大高度（px），一般由 window.innerHeight * COMPOSER_MAX_RATIO 得出 */
  max: number;
}

/**
 * 由指针绝对位移求输入区高度：startH +（锚点 y − 当前 y），向上拖为正增量，钳位到 [min, max]。
 * 纯函数、无副作用、结果只取决于入参，故同位置多帧不会漂移。
 */
export function computeComposerHeight({ startH, anchorY, clientY, min, max }: ComposerHeightArgs): number {
  const raw = startH + (anchorY - clientY);
  return Math.min(max, Math.max(min, raw));
}
