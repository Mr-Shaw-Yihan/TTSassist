// 点击气泡播放按钮时的动作决策（纯函数）。见 playAction.test.ts 行为契约。

export type PlayAction = "stop" | "play";

export function resolvePlayAction(args: {
  playingPath: string | null;
  target: string;
  /** 前端 <audio> 正在播 target */
  localPlaying: boolean;
  /** 宿主流式正在播 target */
  hostPlaying: boolean;
}): PlayAction {
  const { playingPath, target, localPlaying, hostPlaying } = args;
  if (playingPath === target && (localPlaying || hostPlaying)) return "stop";
  return "play";
}
