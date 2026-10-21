// 「我的音色」按能力显示（B5）。
// 契约：manifest.json 的 capabilities.voice_library，缺省视为 false（见计划书跨线契约 3）。
// 为什么要有这道能力位：没有云端音色库的引擎在界面上摆一个「我的音色」保存框，
// 用户照着说明去别处复制音色 ID 也永远用不上，纯属误导。
// 本项目的基座合同禁止按引擎 id 写死判断，所以一律读插件自述的能力位。

import { describe, it, expect } from "vitest";
import { hasVoiceLibrary } from "./pluginCapability";
import voiceSynthPanelSrc from "../components/Voice/VoiceSynthPanel.tsx?raw";

describe("hasVoiceLibrary", () => {
  it("显式声明 true 时有音色库", () => {
    expect(hasVoiceLibrary({ capabilities: { voice_library: true } })).toBe(true);
  });

  it("显式声明 false 时无音色库", () => {
    expect(hasVoiceLibrary({ capabilities: { voice_library: false } })).toBe(false);
  });

  it("老插件的 manifest 根本没有 capabilities 字段时按 false（缺省即无）", () => {
    expect(hasVoiceLibrary({})).toBe(false);
  });

  it("有 capabilities 对象但没写 voice_library 时也按 false", () => {
    expect(hasVoiceLibrary({ capabilities: {} })).toBe(false);
  });

  it("字段为 null / undefined 时按 false（后端缺省序列化差异不该炸界面）", () => {
    expect(hasVoiceLibrary({ capabilities: null })).toBe(false);
    expect(hasVoiceLibrary({ capabilities: undefined })).toBe(false);
  });
});

describe("语音合成面板的渲染条件", () => {
  it("「我的音色」区只在有音色库能力时渲染", () => {
    // 能力判断必须紧邻 CustomVoicePanel，防止有人把它改回无条件渲染
    expect(voiceSynthPanelSrc).toMatch(/hasVoiceLibrary\(cur\)[\s\S]{0,400}<CustomVoicePanel/);
  });
});
