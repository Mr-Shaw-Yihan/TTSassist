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
  /** 自定义快捷播放快捷键（如 "Alt+1"），未设置为 null */
  hotkey: string | null;
}

/** Moss-TTS 音色条目 */
export interface MossVoice {
  name: string;
  voice_id: string;
}

/** Edge-TTS 音色条目 */
export interface EdgeVoiceItem {
  id: string;
  label: string;
}

export interface Settings {
  tts_engine: string;
  tts_model: string;
  playback_volume: number;
  hotkey_show_window: string;
  engine_category: string;
  mimo_api_key: string;
  /** Edge TTS 音色（免费引擎），如 "zh-CN-XiaoxiaoNeural" */
  edge_voice: string;
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
  /** 虚拟麦克风输出设备名（空=未配置） */
  mic_output_device: string;
  /** 全局开关：发送的语音是否同时发到虚拟麦克风 */
  mic_send_enabled: boolean;
  /** 虚拟麦克风音量 0.0~1.0 */
  mic_playback_volume: number;
  /** 各插件引擎当前选中的音色（插件 id → 音色 id） */
  plugin_voices: Record<string, string>;
}

/** 插件音色条目 */
export interface PluginVoiceItem {
  id: string;
  label: string;
}

/** 已安装插件信息（list_plugins 返回） */
export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  /** 是否加载成功可用 */
  loaded: boolean;
  /** 加载失败原因（loaded=false 时有值） */
  error: string | null;
  voices: PluginVoiceItem[];
  /** 音频格式（如 mp3） */
  audio_format: string;
}

/** 音频输出设备 */
export interface AudioDevice {
  name: string;
  is_virtual_cable: boolean;
  is_default: boolean;
}

/** 虚拟麦克风播放状态 */
export interface MicStatus {
  is_playing: boolean;
  current_device: string | null;
  volume: number;
  last_error: string | null;
  last_source: string | null;
}