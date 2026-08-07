// 麦克风录音工具：采集 PCM 并编码为 WAV（16kHz / 16bit / 单声道）。
//
// 为什么不用 MediaRecorder：WebView2 默认产出 webm/opus，ASR 插件只认 wav/mp3，
// 故用 Web Audio API 直接采 PCM，手动编码 WAV（体积小：约 32KB/秒）。
//
// 用法：
//   const rec = new AudioRecorder();
//   await rec.start();        // 申请麦克风权限并开始采集
//   ...
//   const wav = await rec.stop();  // 停止并返回 WAV 字节
//   rec.cancel();             // 放弃录音（释放资源，不产出数据）

const TARGET_SAMPLE_RATE = 16000;

export class AudioRecorder {
  private stream: MediaStream | null = null;
  private ctx: AudioContext | null = null;
  private source: MediaStreamAudioSourceNode | null = null;
  private processor: ScriptProcessorNode | null = null;
  private chunks: Float32Array[] = [];

  get recording(): boolean {
    return this.processor !== null;
  }

  /** 开始录音（申请麦克风权限；拒绝时抛错，文案可直接展示） */
  async start(): Promise<void> {
    if (this.recording) return;
    this.chunks = [];

    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
      });
    } catch (e) {
      const err = e as DOMException;
      if (err.name === "NotAllowedError" || err.name === "PermissionDeniedError") {
        throw new Error("麦克风权限被拒绝，请在系统设置中允许本应用使用麦克风");
      }
      if (err.name === "NotFoundError") {
        throw new Error("未检测到麦克风设备");
      }
      throw new Error(`无法打开麦克风：${err.message || err.name}`);
    }

    const ctx = new AudioContext();
    const source = ctx.createMediaStreamSource(stream);
    // ScriptProcessorNode 已废弃但 WebView2 稳定可用；4096 帧一个回调块
    const processor = ctx.createScriptProcessor(4096, 1, 1);
    processor.onaudioprocess = (ev) => {
      // 拷贝一份（inputBuffer 会被复用）
      this.chunks.push(new Float32Array(ev.inputBuffer.getChannelData(0)));
    };
    source.connect(processor);
    processor.connect(ctx.destination);

    this.stream = stream;
    this.ctx = ctx;
    this.source = source;
    this.processor = processor;
  }

  /** 停止录音，返回 WAV 字节（16kHz/16bit/mono，降采样自设备采样率） */
  async stop(): Promise<Uint8Array> {
    if (!this.ctx || !this.processor) {
      throw new Error("录音未开始");
    }
    const inputRate = this.ctx.sampleRate;
    const chunks = this.chunks;
    this.cleanup();

    // 拼接所有 PCM 分片
    const totalLen = chunks.reduce((n, c) => n + c.length, 0);
    if (totalLen === 0) {
      throw new Error("未采集到音频，请确认麦克风正常后重试");
    }
    const merged = new Float32Array(totalLen);
    let offset = 0;
    for (const c of chunks) {
      merged.set(c, offset);
      offset += c.length;
    }

    const pcm = downsample(merged, inputRate, TARGET_SAMPLE_RATE);
    return encodeWav(pcm, TARGET_SAMPLE_RATE);
  }

  /** 放弃录音，仅释放资源 */
  cancel(): void {
    this.chunks = [];
    this.cleanup();
  }

  /** 释放麦克风与音频上下文 */
  private cleanup(): void {
    this.processor?.disconnect();
    this.processor = null;
    this.source?.disconnect();
    this.source = null;
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = null;
    // AudioContext.close 是异步的，不等待也无妨
    this.ctx?.close().catch(() => {});
    this.ctx = null;
  }
}

/** 线性降采样（块内平均，抑制混叠） */
function downsample(buffer: Float32Array, inputRate: number, outputRate: number): Float32Array {
  if (inputRate === outputRate) return buffer;
  const ratio = inputRate / outputRate;
  const newLength = Math.round(buffer.length / ratio);
  const result = new Float32Array(newLength);
  for (let i = 0; i < newLength; i++) {
    const start = Math.floor(i * ratio);
    const end = Math.min(Math.floor((i + 1) * ratio), buffer.length);
    let sum = 0;
    let count = 0;
    for (let j = start; j < end; j++) {
      sum += buffer[j];
      count++;
    }
    result[i] = count > 0 ? sum / count : 0;
  }
  return result;
}

/** Float32 PCM → WAV 字节（44 字节 RIFF 头 + PCM16 数据） */
function encodeWav(samples: Float32Array, sampleRate: number): Uint8Array {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);

  const writeString = (offset: number, str: string) => {
    for (let i = 0; i < str.length; i++) view.setUint8(offset + i, str.charCodeAt(i));
  };

  writeString(0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true); // 文件总长 - 8
  writeString(8, "WAVE");
  writeString(12, "fmt ");
  view.setUint32(16, 16, true); // fmt 块大小
  view.setUint16(20, 1, true); // PCM 格式
  view.setUint16(22, 1, true); // 单声道
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // 字节率 = 采样率 × 2字节
  view.setUint16(32, 2, true); // 块对齐
  view.setUint16(34, 16, true); // 位深
  writeString(36, "data");
  view.setUint32(40, samples.length * 2, true);

  let offset = 44;
  for (let i = 0; i < samples.length; i++, offset += 2) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return new Uint8Array(buffer);
}
