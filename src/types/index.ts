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

export interface Settings {
  tts_engine: string;
  tts_model: string;
  playback_volume: number;
  hotkey_show_window: string;
  engine_category: string;
  mimo_api_key: string;
  playback_rate: number;
}