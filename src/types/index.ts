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
  /** 虚拟麦克风输出设备名（空=未配置） */
  mic_output_device: string;
  /** 全局开关：发送的语音是否同时发到虚拟麦克风 */
  mic_send_enabled: boolean;
  /** 虚拟麦克风音量 0.0~1.0 */
  mic_playback_volume: number;
  /** 各插件引擎当前选中的音色（插件 id → 音色 id） */
  plugin_voices: Record<string, string>;
  /** 用户选择"忽略"的更新版本号（空=未忽略） */
  update_ignored_version: string;
  /** 当前选择的 ASR 插件 id（空=未配置，自动用第一个可用的） */
  asr_plugin: string;
  /** ASR 识别语言（auto/zh/en） */
  asr_language: string;
  /** 语音输入全局快捷键（空=未设置） */
  voice_input_hotkey: string;
  /** 语音输入功能总开关 */
  voice_input_enabled: boolean;
}

/** 版本更新信息（check_app_update 返回） */
export interface UpdateInfo {
  /** 新版本号（不带 v） */
  version: string;
  /** Release 页面地址 */
  url: string;
  /** 更新说明 */
  notes: string;
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

/** 官方插件索引条目（fetch_plugin_index 返回） */
export interface PluginIndexEntry {
  id: string;
  name: string;
  version: string;
  download_url: string;
  /** zip 包的 SHA-256 */
  checksum: string;
  description: string;
}

/** 内置插件条目（随安装包携带，list_bundled_plugins 返回） */
export interface BundledPluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  /** 本机是否已安装 */
  installed: boolean;
}

/** ASR 插件信息（list_asr_plugins 返回） */
export interface AsrPluginInfo {
  id: string;
  name: string;
  version: string;
  /** 是否加载成功可用 */
  loaded: boolean;
  /** 支持语言 JSON 字符串，如 [{"code":"zh","label":"中文"}] */
  languages: string;
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