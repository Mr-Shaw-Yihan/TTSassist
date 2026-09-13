// audioRecorder 单测（T3 优先级 3）：采样率/字节数换算与时长的数值等价断言。
// 不碰真实麦克风：只测纯函数 downsample / encodeWav（export 为 T3 授权的可见性放宽）。

import { describe, it, expect } from "vitest";
import { downsample, encodeWav } from "./audioRecorder";

describe("downsample", () => {
  it("同采样率时原样返回", () => {
    const buf = new Float32Array([0.1, 0.2, 0.3]);
    expect(downsample(buf, 16000, 16000)).toBe(buf);
  });

  it("48k→16k 三点合一取平均", () => {
    const buf = new Float32Array([0.0, 0.3, 0.9, 0.0, 0.3, 0.9]);
    const out = downsample(buf, 48000, 16000);
    expect(out.length).toBe(2);
    expect(out[0]).toBeCloseTo(0.4, 6);
    expect(out[1]).toBeCloseTo(0.4, 6);
  });

  it("输出长度 = round(输入长 / 比率)", () => {
    const buf = new Float32Array(44101);
    const out = downsample(buf, 44100, 16000);
    expect(out.length).toBe(Math.round(44101 / (44100 / 16000)));
  });
});

describe("encodeWav", () => {
  it("头部 44 字节 + PCM16 数据（2 字节/样本）", () => {
    const wav = encodeWav(new Float32Array([0, 1, -1]), 16000);
    expect(wav.length).toBe(44 + 3 * 2);
    // RIFF/WAVE 魔数
    expect(String.fromCharCode(...wav.slice(0, 4))).toBe("RIFF");
    expect(String.fromCharCode(...wav.slice(8, 12))).toBe("WAVE");
    // 单声道 PCM 16bit
    expect(wav[22]).toBe(1); // 声道数
    expect(wav[34]).toBe(16); // 位深
    // 采样率 16000（LE 32 位）
    const dv = new DataView(wav.buffer, wav.byteOffset, wav.byteLength);
    expect(dv.getUint32(24, true)).toBe(16000);
    // 字节率 = 采样率 × 2
    expect(dv.getUint32(28, true)).toBe(32000);
    // 时长换算：data 块字节数 / 2 / 采样率 = 秒
    const dataBytes = dv.getUint32(40, true);
    const seconds = dataBytes / 2 / 16000;
    expect(seconds).toBeCloseTo(3 / 16000, 9);
  });

  it("振幅换算：0→0，1→32767，-1→-32768，越界截断", () => {
    const dv = new DataView(encodeWav(new Float32Array([0, 1, -1, 2, -2]), 16000).buffer);
    expect(dv.getInt16(44 + 0 * 2, true)).toBe(0);
    expect(dv.getInt16(44 + 1 * 2, true)).toBe(32767);
    expect(dv.getInt16(44 + 2 * 2, true)).toBe(-32768);
    expect(dv.getInt16(44 + 3 * 2, true)).toBe(32767); // 2 → 截到 1
    expect(dv.getInt16(44 + 4 * 2, true)).toBe(-32768); // -2 → 截到 -1
  });

  it("10 秒静音的体积符合 32KB/秒 口径", () => {
    const wav = encodeWav(new Float32Array(16000 * 10), 16000);
    // 44 头 + 16000*10 样本*2字节 = 320044 ≈ 32KB/秒（与模块头注释口径一致）
    expect(wav.length).toBe(44 + 16000 * 10 * 2);
  });
});
