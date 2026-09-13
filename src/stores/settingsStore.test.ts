// settingsStore 单测（T3 优先级 2）：mock 掉 @tauri-apps/api 链路的 updateSetting，
// 断言 patch 后本地态与调用参数正确；另设「Settings 字段完整性」测试，
// 挡住「后端加字段前端忘同步」那一类回归（v1.8.2 事故的前端翻版）。

import { describe, it, expect, vi, beforeEach } from "vitest";
import { useSettingsStore } from "./settingsStore";
import type { Settings } from "../types";

// mock 相对 settingsStore 的同一路径模块：../services/invoke
vi.mock("../services/invoke", () => ({
  updateSetting: vi.fn(async (_key: string, value: unknown) => value as Settings),
}));

import { updateSetting } from "../services/invoke";
const mockedUpdate = vi.mocked(updateSetting);

/** 前端权威（types/index.ts 的 Settings）字段镜像清单。
 *  注意：后端 storage/types.rs 还有 minimax_api_key / minimax_global_api_key 两个字段
 *  未出现在前端 types（历史遗留，见交付报告可疑点）——本清单以 types 为准，
 *  后端/前端任一侧加字段时同步这里，漏一处测试即红。 */
const EXPECTED_KEYS: (keyof Settings)[] = [
  "tts_engine",
  "tts_model",
  "playback_volume",
  "hotkey_show_window",
  "engine_category",
  "mimo_api_key",
  "playback_rate",
  "clone_voice_name",
  "clone_voice_path",
  "theme",
  "moss_api_key",
  "minimax_global_cloned_voices",
  "moss_voice_id",
  "moss_voices",
  "mic_output_device",
  "mic_send_enabled",
  "mic_playback_volume",
  "plugin_voices",
  "update_ignored_version",
  "asr_plugin",
  "asr_language",
  "voice_input_hotkey",
  "voice_input_enabled",
  "voice_input_device",
  "hotkey_play_last",
  "hotkey_mic_toggle",
  "plugin_config",
  "floating_ball_enabled",
  "floating_ball_x",
  "floating_ball_y",
  "floating_ball_size",
  "floating_ball_perf_mode",
  "floating_ball_skin",
  "diagnostics_log_enabled",
  "subtitle_enabled",
  "subtitle_always_on_top",
  "subtitle_opacity",
  "subtitle_font_size",
  "subtitle_position",
  "subtitle_target_process",
  "subtitle_asr_plugin",
  "subtitle_language",
  "subtitle_vad_sensitivity",
  "subtitle_pause_hotkey",
  "subtitle_max_lines",
  "subtitle_fade_seconds",
];

/** 完整构造（TS 编译期保证与 types/index.ts 一致）：漏字段/多字段这里就编译不过 */
function makeSettings(): Settings {
  return {
    tts_engine: "edge-tts",
    tts_model: "default",
    playback_volume: 0.8,
    hotkey_show_window: "Alt+V",
    engine_category: "remote",
    mimo_api_key: "",
    playback_rate: 1.0,
    clone_voice_name: "",
    clone_voice_path: "",
    theme: "light",
    moss_api_key: "",
    minimax_global_cloned_voices: [],
    moss_voice_id: "11d3a27a-0951-4bcd-96ae-ab414f454764",
    moss_voices: [{ name: "曼波", voice_id: "11d3a27a-0951-4bcd-96ae-ab414f454764" }],
    mic_output_device: "",
    mic_send_enabled: false,
    mic_playback_volume: 1.0,
    plugin_voices: { "edge-tts": "zh-CN-XiaoxiaoNeural" },
    update_ignored_version: "",
    asr_plugin: "mimo-asr",
    asr_language: "zh",
    voice_input_hotkey: "",
    voice_input_enabled: true,
    voice_input_device: "",
    hotkey_play_last: "",
    hotkey_mic_toggle: "",
    plugin_config: {},
    floating_ball_enabled: false,
    floating_ball_x: -1,
    floating_ball_y: -1,
    floating_ball_size: 56,
    floating_ball_perf_mode: "standard",
    floating_ball_skin: "ink",
    diagnostics_log_enabled: false,
    subtitle_enabled: false,
    subtitle_always_on_top: true,
    subtitle_opacity: 0.6,
    subtitle_font_size: 18,
    subtitle_position: "bottom",
    subtitle_target_process: "",
    subtitle_asr_plugin: "",
    subtitle_language: "auto",
    subtitle_vad_sensitivity: "medium",
    subtitle_pause_hotkey: "Alt+M",
    subtitle_max_lines: 3,
    subtitle_fade_seconds: 15,
  };
}

describe("settingsStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({ settings: null });
  });

  it("初始为 null，setSettings 后为对象", () => {
    expect(useSettingsStore.getState().settings).toBeNull();
    const s = makeSettings();
    useSettingsStore.getState().setSettings(s);
    expect(useSettingsStore.getState().settings).toEqual(s);
  });

  it("patch 把后端返回的完整 settings 写入本地态", async () => {
    const s = makeSettings();
    const updated = { ...s, playback_volume: 0.5 };
    mockedUpdate.mockResolvedValueOnce(updated);
    await useSettingsStore.getState().patch("playback_volume", 0.5);
    expect(mockedUpdate).toHaveBeenCalledWith("playback_volume", 0.5);
    expect(useSettingsStore.getState().settings).toEqual(updated);
    expect(useSettingsStore.getState().settings?.playback_volume).toBe(0.5);
  });

  it("patch 失败时不上屏（本地态保持旧值）", async () => {
    const s = makeSettings();
    useSettingsStore.getState().setSettings(s);
    mockedUpdate.mockRejectedValueOnce(new Error("写入失败"));
    await expect(useSettingsStore.getState().patch("theme", "dark")).rejects.toThrow("写入失败");
    expect(useSettingsStore.getState().settings).toEqual(s);
  });

  it("Settings 字段完整性：镜像清单被 makeSettings 全部覆盖", () => {
    const keys = Object.keys(makeSettings());
    const missing = EXPECTED_KEYS.filter((k) => !keys.includes(k));
    expect(missing).toEqual([]);
    // 顺序无关的总数一致性：types 多出清单外字段同样视为失同步
    expect(keys.length).toBe(EXPECTED_KEYS.length);
  });
});
