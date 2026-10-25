// 插件配置卡的两条真机回归守卫（10-26 用户复报与子代理附带发现）。

import { describe, expect, it } from "vitest";
import panelSrc from "./PluginConfigPanel.tsx?raw";
import voiceSynthSrc from "../Voice/VoiceSynthPanel.tsx?raw";

describe("必填校验不误报已有值的密钥", () => {
  it("secret 已有值时不算缺填（否则会与占位「已设置」同屏矛盾）", () => {
    // display 不算、空 draft 才算缺填，但「secret 且服务端已有值」必须豁免
    expect(panelSrc).toMatch(/f\.type !== "display" &&/);
    expect(panelSrc).toMatch(/!\(f\.type === "secret" && hadSecret\[f\.key\]\)/);
  });
});

describe("切换引擎时配置卡重挂", () => {
  it("配置卡按引擎 id 加 key（否则输入框复用，上一个引擎的值残留）", () => {
    expect(voiceSynthSrc).toMatch(
      /<PluginConfigPanel key=\{cur\.id\} pluginId=\{cur\.id\} pluginName=\{cur\.name\} title="API 密钥" \/>/,
    );
  });
});
