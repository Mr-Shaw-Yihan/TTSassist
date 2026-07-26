// 前端类型定义，与 Rust 后端的 serde 序列化结构对应。

export interface Message {
  id: string;
  content: string;
  /** 相对 app_data_dir 的音频路径，如 "audio/m_xxx.wav" */
  audio_path: string;
  /** ISO8601 时间戳字符串 */
  created_at: string;
}

export interface Favorite {
  id: string;
  source_message_id: string | null;
  note: string;
  audio_path: string;
  created_at: string;
}

/** Moss-TTS 音色条目 */
export interface MossVoice {
  name: string;
  voice_id: string;
}

export interface Settings {
  tts_engine: string;
  tts_model: string;
  playback_volume: number;
  hotkey_show_window: string;
  engine_category: string;
  mimo_api_key: string;
  playback_rate: number;
  clone_voice_name: string;
  clone_voice_path: string;
  /** 皮肤：light（安墨）/ dark（夜窗） */
  theme: string;
  /** Moss-TTS API Key */
  moss_api_key: string;
  /** Moss-TTS 当前选中音色 id */
  moss_voice_id: string;
  /** Moss-TTS 音色库（用户手动维护） */
  moss_voices: MossVoice[];
}