// accelerator 单测（T3 优先级 1）：键盘事件 → 加速键串。
// 该模块决定用户能绑什么键，错了就是「按了没反应/绑不上」类报障源头。
// 注意：冲突判定在后端 hotkey.rs::find_accel_conflict，前端无此函数，不测。

import { describe, it, expect } from "vitest";
import { mapKey, buildAccelerator } from "./accelerator";

/** 构造键盘事件形状的小工具 */
const ev = (key: string, m: Partial<{ ctrl: boolean; alt: boolean; shift: boolean; meta: boolean }> = {}) => ({
  key,
  ctrlKey: !!m.ctrl,
  altKey: !!m.alt,
  shiftKey: !!m.shift,
  metaKey: !!m.meta,
});

describe("mapKey", () => {
  it("空格映射为 Space（修复：特判须先于单字符分支，原 case 为死分支）", () => {
    expect(mapKey(" ")).toBe("Space");
  });

  it("四个方向键映射为 Up/Down/Left/Right", () => {
    expect(mapKey("ArrowUp")).toBe("Up");
    expect(mapKey("ArrowDown")).toBe("Down");
    expect(mapKey("ArrowLeft")).toBe("Left");
    expect(mapKey("ArrowRight")).toBe("Right");
  });

  it("Escape/Enter/Tab/Backspace/Delete 保持原样", () => {
    expect(mapKey("Escape")).toBe("Escape");
    expect(mapKey("Enter")).toBe("Enter");
    expect(mapKey("Tab")).toBe("Tab");
    expect(mapKey("Backspace")).toBe("Backspace");
    expect(mapKey("Delete")).toBe("Delete");
  });

  it("F1~F12 保持原样", () => {
    for (let i = 1; i <= 12; i++) expect(mapKey(`F${i}`)).toBe(`F${i}`);
  });

  it("单字符大写化：字母转大写、数字不变", () => {
    expect(mapKey("v")).toBe("V");
    expect(mapKey("V")).toBe("V");
    expect(mapKey("1")).toBe("1");
  });

  it("非 ASCII 单字符走 toUpperCase（钉住现状，不改行为）", () => {
    // é 在某些 locale 下 toUpperCase() 为 É；土耳其 i 等 locale 相关行为浏览器自负。
    // 这里只钉住实现实际产物：等价于 key.toUpperCase()
    expect(mapKey("é")).toBe("é".toUpperCase());
    expect(mapKey("文")).toBe("文".toUpperCase());
  });
});

describe("buildAccelerator", () => {
  it("仅按修饰键（无主键）返回 null", () => {
    for (const key of ["Control", "Alt", "Shift", "Meta"]) {
      expect(buildAccelerator(ev(key, { ctrl: true, alt: true, shift: true, meta: true }))).toBeNull();
    }
  });

  it("修饰键顺序固定 Ctrl→Alt→Shift→Meta", () => {
    // 与后端 hotkey.rs::find_accel_conflict 的归一化假定一致
    expect(buildAccelerator(ev("v", { shift: true, meta: true, ctrl: true, alt: true }))).toBe("Ctrl+Alt+Shift+Meta+V");
    expect(buildAccelerator(ev("F2", { shift: true, ctrl: true }))).toBe("Ctrl+Shift+F2");
    expect(buildAccelerator(ev("1", { alt: true }))).toBe("Alt+1");
  });

  it("无修饰键时只有主键", () => {
    expect(buildAccelerator(ev("V"))).toBe("V");
    expect(buildAccelerator(ev("Space"))).toBe("Space");
  });

  it("主键为 + 时拒绑返回 null（T-C 裁决：分隔符冲突且 global-hotkey 无 Plus 键码）", () => {
    expect(buildAccelerator(ev("+", { ctrl: true }))).toBeNull();
    // Shift+= 产出的同样是 "+"，一并拒绑
    expect(buildAccelerator(ev("+", { ctrl: true, shift: true }))).toBeNull();
  });

  it("Ctrl 与 = 组合正常产出 Ctrl+=（+ 键的替代绑定路径）", () => {
    expect(buildAccelerator(ev("=", { ctrl: true }))).toBe("Ctrl+=");
  });

  it("空格主键带修饰键拼出 Ctrl+Space（修复后）", () => {
    expect(buildAccelerator(ev(" ", { ctrl: true }))).toBe("Ctrl+Space");
  });
});
