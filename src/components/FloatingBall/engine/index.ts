// 引擎入口：按依赖序加载 grok 复刻引擎各层（IIFE 挂载 window），对外暴露轻量门面。
// 演示期使用 grok 素材（仅本机，不进发布包）；换原创素材时只替换本目录文件，
// 门面接口（setState/spinOnce/…）保持不变，上层编排不动。
import "./geometry-data.js";
import "./math.js";
import "./tables.js";
import "./pose.js";
import "./tricks.js";
import "./fx.js";
import "./eyes.js";
import "./character.js";

/** 角色门面：只暴露电子声带编排用到的能力，隔离引擎内部 API */
export interface BallCharacter {
  setState(name: string, opts?: { resetEyes?: boolean }): void;
  /** 转一圈（不含粒子——粒子是独立的 burstOnce） */
  spinOnce(turns?: number): void;
  bounceOnce(): void;
  burstOnce(): void;
  setPaused(v: boolean): void;
  /** 帧率封顶：0=不封顶；30=性能模式 */
  setFpsCap(n: number): void;
  /** 外部喂视线目标（客户区坐标）；null=解除跟随 */
  setGazeTarget(pt: { x: number; y: number } | null): void;
  setColor(id: string, scheme?: string): void;
  /** 平色墨色（绕过 light-dark()，部分 webview 不支持会导致白体） */
  setInk(flat: string): void;
  /** 眼睛填充色；null=回退引擎默认奶油色 */
  setEyeColor(color: string | null): void;
  setFollowPointer(v: boolean): void;
  destroy(): void;
}

interface WindowWithGrok extends Window {
  GrokCharacter: new (svg: SVGSVGElement, opts?: Record<string, unknown>) => BallCharacter;
}

/** 创建角色实例。scheme: light/dark；shape/color 用引擎内置词表（演示期 blob/black） */
export function createBallCharacter(
  svg: SVGSVGElement,
  opts: { scheme?: string; followPointer?: boolean } = {},
): BallCharacter {
  const Ctor = (window as unknown as WindowWithGrok).GrokCharacter;
  return new Ctor(svg, {
    shape: "blob",
    color: "black",
    scheme: opts.scheme ?? "light",
    // live 模式：关闭登录页的情绪轮播（onboarding 每 1.2s 切状态，不适合常驻球）
    mode: "live",
    loginWrap: false,
    // 全局指针跟随由后端光标轮询 + setGazeTarget 驱动（webview 自身只能感知窗内指针）
    followPointer: false,
    state: "idle",
    // 引擎内置徽标（notifying 叠加）默认蓝色，统一改绿与 React 徽标一致
    badgeColor: "#22c55e",
  });
}
