// 点击气泡播放按钮时的动作决策（纯函数）。
// 回归源头（用户真机报出 bug④）：宿主流式合成已在后台播这条音频，但气泡仍显示「播放」
// （前端 playingPath 没同步宿主播放态），用户一点 → 又起一段本地播放 = 双播/从头重播。
// 修法：宿主开播把这条标成「播放中」，再点它应是「停止」而不是「再播」。

import { describe, it, expect } from "vitest";
import { resolvePlayAction } from "./playAction";

describe("resolvePlayAction", () => {
  it("未在播该条 → play", () => {
    expect(
      resolvePlayAction({ playingPath: null, target: "audio/a.wav", localPlaying: false, hostPlaying: false }),
    ).toBe("play");
  });

  it("前端 <audio> 正在播该条 → stop（再点即停，与旧行为一致）", () => {
    expect(
      resolvePlayAction({ playingPath: "audio/a.wav", target: "audio/a.wav", localPlaying: true, hostPlaying: false }),
    ).toBe("stop");
  });

  it("宿主流式正在播该条 → stop（修 bug④：点它不再从头重播，而是停）", () => {
    expect(
      resolvePlayAction({ playingPath: "audio/a.wav", target: "audio/a.wav", localPlaying: false, hostPlaying: true }),
    ).toBe("stop");
  });

  it("高亮停在该条但两路都没在播（陈旧态）→ play（当作重新播放）", () => {
    expect(
      resolvePlayAction({ playingPath: "audio/a.wav", target: "audio/a.wav", localPlaying: false, hostPlaying: false }),
    ).toBe("play");
  });

  it("播的是另一条 → play（互斥切换）", () => {
    expect(
      resolvePlayAction({ playingPath: "audio/b.wav", target: "audio/a.wav", localPlaying: true, hostPlaying: false }),
    ).toBe("play");
  });
});
