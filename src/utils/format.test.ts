// 体积格式化的行为契约。设置页与卸载弹窗都靠它向用户报"占了多大"，
// 报错数量级比报错本身更糟——所以进位边界与异常值都要钉住。

import { describe, it, expect } from "vitest";
import { formatBytes } from "./format";

describe("formatBytes", () => {
  it("零与常见进位", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1023)).toBe("1023 B"); // 还没到 1 KB
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  it("GB 级保留一位小数", () => {
    expect(formatBytes(3.2 * 1024 ** 3)).toBe("3.2 GB");
    expect(formatBytes(1024 ** 3)).toBe("1 GB");
  });

  it("异常值不产出 NaN 或 undefined", () => {
    // 原实现只挡 0：负数会算出 "-Infinity B"，TB 级会取到不存在的单位变成 "NaN undefined"。
    // 急救与卸载弹窗拿到的都是磁盘真实数字，宁可显示得保守，也不能显示乱码级结果。
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
    expect(formatBytes(2 * 1024 ** 4)).toBe("2048 GB"); // 超出最大单位时停在 GB，不越界
  });
});
