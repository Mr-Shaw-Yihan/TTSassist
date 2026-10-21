// 首启弹窗挂载守卫（B1）。
// A9 交付的 StorageFirstRunDialog 自管 pending 判定，宿主侧的职责只有三条：
// 挂上、不传 props（传了就是把判定权又收回 App）、挂载点落在皮肤 effect 之后且早于 Tab 内容
// ——否则它嵌进某个页签里，切页就把自己卸载了，首启询问也就丢了。
//
// 环境说明：vitest 是 node 环境、无 @testing-library/react，无法渲染 App 断言弹窗出现，
// 故对 App.tsx 源码做结构守卫（渲染顺序即源码顺序）。弹窗自身的显示/隐藏逻辑由
// A 的 StorageFirstRunDialog.test.ts 覆盖。

import { describe, it, expect } from "vitest";
import appSrc from "./App.tsx?raw";

describe("App.tsx 挂载首启数据位置弹窗", () => {
  it("导入了组件", () => {
    expect(appSrc).toMatch(
      /import\s*\{\s*StorageFirstRunDialog\s*\}\s*from\s*"\.\/components\/Storage\/StorageFirstRunDialog"/,
    );
  });

  it("以无 props 形式挂载（组件自管 pending 判定）", () => {
    expect(appSrc).toContain("<StorageFirstRunDialog />");
  });

  it("挂载点在皮肤 effect 之后", () => {
    const theme = appSrc.indexOf('setAttribute("data-theme"');
    const mount = appSrc.indexOf("<StorageFirstRunDialog />");
    expect(theme).toBeGreaterThan(-1);
    expect(mount).toBeGreaterThan(theme);
  });

  it("挂载点在 Tab 内容之前（不能嵌进页签里，切页会把它卸载）", () => {
    const mount = appSrc.indexOf("<StorageFirstRunDialog />");
    const firstTab = appSrc.indexOf('{tab === "messages"');
    expect(mount).toBeGreaterThan(-1); // 未挂载时 indexOf 是 -1，会让下面这条比较假绿
    expect(firstTab).toBeGreaterThan(-1);
    expect(mount).toBeLessThan(firstTab);
  });
});
