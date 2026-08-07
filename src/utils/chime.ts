// 叮咚提示音：Web Audio 振荡器合成，无需音频素材文件。
// 开始录音 = 上行两音（叮~咚），结束录音 = 下行两音（咚~叮），听感可区分起止。

let ctx: AudioContext | null = null;

function audioCtx(): AudioContext {
  if (!ctx) ctx = new AudioContext();
  // 窗口未交互过可能被挂起，恢复一下
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

/** 单个正弦音：快起慢落的包络，避免爆音 */
function tone(ac: AudioContext, freq: number, at: number, dur: number) {
  const osc = ac.createOscillator();
  const gain = ac.createGain();
  osc.type = "sine";
  osc.frequency.value = freq;
  gain.gain.setValueAtTime(0.0001, at);
  gain.gain.exponentialRampToValueAtTime(0.22, at + 0.02);
  gain.gain.exponentialRampToValueAtTime(0.0001, at + dur);
  osc.connect(gain);
  gain.connect(ac.destination);
  osc.start(at);
  osc.stop(at + dur + 0.05);
}

/** 上行叮咚（E5→A5）：录音开始 */
export function playStartChime() {
  try {
    const ac = audioCtx();
    const t = ac.currentTime;
    tone(ac, 659.25, t, 0.16);
    tone(ac, 880.0, t + 0.16, 0.22);
  } catch {
    /* 音频不可用时静默 */
  }
}

/** 下行叮咚（A5→E5）：录音结束 */
export function playEndChime() {
  try {
    const ac = audioCtx();
    const t = ac.currentTime;
    tone(ac, 880.0, t, 0.16);
    tone(ac, 659.25, t + 0.16, 0.22);
  } catch {
    /* 音频不可用时静默 */
  }
}
