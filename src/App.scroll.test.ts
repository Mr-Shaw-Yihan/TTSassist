// 消息页滚动守卫（10-26 第四版二轮简化）。
//
// 前三版都被真机推翻：绝对 scrollTop（v1）、距底距离+等两帧（v2）、消息行锚点+逐帧钉位
// 30 帧（v3）。第四版一上来做了 bottom/restore 双意图，真机二轮实测把 restore 也否了：
// 切页回来那一刻列表可能先空一拍，restore 锚点一次落空就被「缺行兜底」立即释放、不再
// 重试，中段恢复全部落到顶部；而 bottom 意图因「达成也不释放」会被定时器链一路补到
// 成功——贴底路径在真机始终可靠。用户拍板：不做中段恢复，切回消息页一律贴底。
// 取证还发现「滚到最大仍被输入面板遮住」是列表盒子与面板几何重叠（滚动上限不足），
// 任何定位算法都救不了，靠底部垫高自愈兜住。
//
// 本文件钉住简化后的结构，防止退回一次性定位与中段恢复：
//   - 唯一意图 following（跟随最新）：启动/切页回来/发送/定位按钮置位，手势带离底部解除；
//   - 对账幂等可重入，四路触发：意图置位时、messages/合成态变化后、ResizeObserver、
//     退化定时器链（隐藏窗口下定时器仍会跑），滚动事件再兜底；
//   - 底部垫高按 main/面板矩形实测，正常布局恒 0，重叠时把等高空间垫进列表末尾。
// 环境说明：vitest 是 node 环境、无 @testing-library/react，无法真滚动断言，故做源码结构守卫。

import { describe, it, expect } from "vitest";
import appSrc from "./App.tsx?raw";

describe("唯一意图：跟随最新（following）", () => {
  it("意图是布尔 ref，置位/解除只发生在 pinBottom 与手势分支", () => {
    expect(appSrc).toContain("const followingRef = useRef(false);");
    expect(appSrc).toMatch(/followingRef\.current = true;/);
    expect(appSrc).toMatch(/followingRef\.current = near;/);
  });

  it("pinBottom 是启动/切页/发送/定位按钮共用的入口", () => {
    expect(appSrc).toMatch(/const pinBottom = useCallback\(\(\) => \{/);
    expect(appSrc).toMatch(/onClick=\{pinBottom\}/);
    expect(appSrc).toMatch(/pinBottom\(\);/);
  });

  it("挂载消息页即置位跟随（切回一律贴底，用户拍板不做中段恢复）", () => {
    expect(appSrc).toMatch(
      /if \(tab !== "messages"\) return;\s*pinBottom\(\);/,
    );
    // 旧机制不允许回流
    expect(appSrc).not.toContain("initialLoaded");
    expect(appSrc).not.toContain("savedAnchorRef");
    expect(appSrc).not.toContain("firstVisibleRow");
    expect(appSrc).not.toContain("findRow");
  });
});

describe("持续对账（不依赖任何一次写成功）", () => {
  it("reconcileScroll 幂等：跟随中写 scrollHeight，且不因已到底而释放", () => {
    const body = appSrc.slice(
      appSrc.indexOf("const reconcileScroll = useCallback"),
      appSrc.indexOf("const startReconcileChain"),
    );
    expect(body).toMatch(/followingRef\.current\) return;/);
    expect(body).toMatch(/el\.scrollTop = el\.scrollHeight;/);
  });

  it("四路对账触发：退化定时器链 / RO 盒子尺寸 / 消息与合成态变化后 / 滚动事件兜底", () => {
    expect(appSrc).toMatch(/const RECONCILE_DELAYS = \[120, 300, 700, 1500, 3000\] as const;/);
    expect(appSrc).toMatch(/new ResizeObserver\(\(\) => \{/);
    expect(appSrc).toMatch(/\}, \[messages, streamingIds, failedIds, reconcileScroll\]\);/);
    expect(appSrc).toMatch(/if \(followingRef\.current\) reconcileScroll\(\);/);
  });

  it("定时器链按代数失效（新置位让旧链作废；卸载同样置废）", () => {
    expect(appSrc).toMatch(/const gen = generationRef\.current;/);
    expect(appSrc).toMatch(/if \(generationRef\.current === gen\) reconcileScroll\(\);/);
    expect(appSrc).toMatch(/generationRef\.current \+= 1;\s*\},\s*\[\],\s*\);/);
  });
});

describe("手势让位（不与用户抢滚动条）", () => {
  it("滚轮/触摸/指针按下都有监听，指针松开复位", () => {
    expect(appSrc).toMatch(/el\.addEventListener\("wheel", mark, \{ passive: true \}\);/);
    expect(appSrc).toMatch(/el\.addEventListener\("touchmove", mark, \{ passive: true \}\);/);
    expect(appSrc).toMatch(/el\.addEventListener\("pointerdown", down\);/);
    expect(appSrc).toMatch(/window\.addEventListener\("pointerup", up\);/);
  });

  it("手势期间不回写；离开底部停止跟随（↓按钮出现），带回底部恢复跟随", () => {
    expect(appSrc).toMatch(
      /const gesturing =\s*pointerScrollingRef\.current \|\| performance\.now\(\) - lastUserScrollAtRef\.current < 400;/,
    );
    expect(appSrc).toMatch(/if \(gesturing\) \{\s*followingRef\.current = near;\s*return;/);
  });
});

describe("底部遮挡自愈（滚动上限不足时垫高列表末尾）", () => {
  it("main/输入面板矩形实测重叠量，正常布局恒为 0", () => {
    expect(appSrc).toMatch(/function measureBottomGuard\(el: HTMLElement, footerTop: number \| null\): number \{/);
    expect(appSrc).toMatch(
      /const overlap = el\.getBoundingClientRect\(\)\.bottom - footerTop \+ RESIZER_H;/,
    );
  });

  it("垫高元素在列表末尾且由 footerRef 供矩形；垫高上限防把消息区垫空", () => {
    expect(appSrc).toContain('<div ref={bottomGuardRef} aria-hidden style={{ height: 0 }} />');
    expect(appSrc).toMatch(/<footer ref=\{footerRef\}/);
    expect(appSrc).toMatch(/const BOTTOM_GUARD_MAX_RATIO = 0\.6;/);
  });

  it("每次对账与盒子尺寸变化都重算垫高", () => {
    expect(appSrc).toMatch(/const applyBottomGuard = useCallback\(\(\) => \{/);
    const reconcileBody = appSrc.slice(
      appSrc.indexOf("const reconcileScroll = useCallback"),
      appSrc.indexOf("const startReconcileChain"),
    );
    expect(reconcileBody).toContain("applyBottomGuard();");
    const roBody = appSrc.slice(appSrc.indexOf("new ResizeObserver"));
    expect(roBody.indexOf("applyBottomGuard();")).toBeGreaterThan(-1);
  });
});

describe("禁止退回已被真机推翻的旧方案", () => {
  it("不出现 v1-v4 的遗留标识与一次性补写", () => {
    expect(appSrc).not.toContain("savedScrollRef");
    expect(appSrc).not.toContain("savedAtBottomRef");
    expect(appSrc).not.toContain("pinUntilStable");
    expect(appSrc).not.toContain("withStableList");
    expect(appSrc).not.toContain("window.setTimeout(run, 150)");
    expect(appSrc).not.toContain('kind: "restore"');
    expect(appSrc).not.toContain("distFromBottom: el.scrollHeight");
  });
});
