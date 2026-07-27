// 虚拟麦克风播放（修正架构：专用音频线程 + channel 遥控）
//
// 关键点（详见 doc/开发记录.md 阶段 11）：
// - rodio 的 OutputStream 在 rodio 0.20（cpal 0.15）下不是 Send+Sync，
//   不能直接放进 Tauri manage 的状态。所以把它关进一个专用线程，
//   全局状态只持有 channel 的 Sender（天生 Send+Sync）。
// - (OutputStream, OutputStreamHandle) 在同一设备上跨播放保活，设备切换才重建。
// - 每次播放新建 Sink（drop 旧 Sink 即停）。
// - recv_timeout 轮询：既响应 Stop，又检测播放完成。

use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};

/// 发往音频线程的命令
enum MicCommand {
    Play { path: PathBuf, device_name: String, volume: f32 },
    Stop,
}

/// 播放状态（供前端查询）
#[derive(Debug, Clone, serde::Serialize)]
pub struct MicStatus {
    pub is_playing: bool,
    pub current_device: Option<String>,
    pub volume: f32,
}

/// 全局虚拟麦克风控制句柄。只含 Sender + 共享状态，Send+Sync，可 manage。
pub struct MicPlayback {
    cmd_tx: Sender<MicCommand>,
    status: Arc<Mutex<MicStatus>>,
}

impl MicPlayback {
    /// 启动音频线程，返回控制句柄
    pub fn spawn() -> Self {
        let (tx, rx) = channel::<MicCommand>();
        let status = Arc::new(Mutex::new(MicStatus {
            is_playing: false,
            current_device: None,
            volume: 1.0,
        }));
        let status_thread = status.clone();
        std::thread::Builder::new()
            .name("voiceassist-mic".into())
            .spawn(move || audio_thread_loop(rx, status_thread))
            .expect("启动麦克风音频线程失败");
        MicPlayback { cmd_tx: tx, status }
    }

    pub fn play(&self, path: PathBuf, device_name: String, volume: f32) {
        let _ = self.cmd_tx.send(MicCommand::Play { path, device_name, volume });
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(MicCommand::Stop);
    }

    pub fn status(&self) -> MicStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(MicStatus { is_playing: false, current_device: None, volume: 1.0 })
    }
}

/// 音频线程主循环：独占 OutputStream/Sink，不跨线程
fn audio_thread_loop(rx: Receiver<MicCommand>, status: Arc<Mutex<MicStatus>>) {
    let mut stream: Option<(rodio::OutputStream, rodio::OutputStreamHandle)> = None;
    let mut cur_device: Option<String> = None;
    let mut sink: Option<rodio::Sink> = None;

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(MicCommand::Play { path, device_name, volume }) => {
                // 设备变化或无流 → 重建
                let need_new = cur_device.as_deref() != Some(device_name.as_str()) || stream.is_none();
                if need_new {
                    sink = None;
                    stream = None;
                    cur_device = None;
                    if let Some(device) = find_device_by_name(&device_name) {
                        if let Ok((s, h)) = rodio::OutputStream::try_from_device(&device) {
                            stream = Some((s, h));
                            cur_device = Some(device_name.clone());
                        } else {
                            eprintln!("mic: 无法打开设备 {device_name}");
                            continue;
                        }
                    } else {
                        eprintln!("mic: 未找到设备 {device_name}");
                        continue;
                    }
                }

                // 先停旧播放，再在保活的 stream 上新建 sink 播放
                sink = None;
                if let Some((_, handle)) = &stream {
                    match rodio::Sink::try_new(handle) {
                        Ok(new_sink) => match decode_file(&path) {
                            Ok(source) => {
                                new_sink.append(source);
                                new_sink.set_volume(volume.clamp(0.0, 1.0));
                                sink = Some(new_sink);
                                write_status(&status, true, cur_device.clone(), volume);
                            }
                            Err(e) => eprintln!("mic 解码失败: {e}"),
                        },
                        Err(e) => eprintln!("mic 创建 Sink 失败: {e}"),
                    }
                }
            }
            Ok(MicCommand::Stop) => {
                sink = None;
                write_status_playing(&status, false);
            }
            Err(RecvTimeoutError::Timeout) => {
                // 检测播放完成
                if sink.as_ref().map_or(false, |s| s.empty()) {
                    sink = None;
                    write_status_playing(&status, false);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn decode_file(
    path: &std::path::Path,
) -> Result<rodio::Decoder<BufReader<std::fs::File>>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开音频失败: {e}"))?;
    rodio::Decoder::new(BufReader::new(file)).map_err(|e| format!("解码失败: {e}"))
}

fn find_device_by_name(name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();
    host.output_devices()
        .ok()?
        .find(|d| d.name().ok().as_deref() == Some(name))
}

fn write_status(status: &Arc<Mutex<MicStatus>>, is_playing: bool, device: Option<String>, volume: f32) {
    if let Ok(mut s) = status.lock() {
        s.is_playing = is_playing;
        s.current_device = device;
        s.volume = volume;
    }
}

fn write_status_playing(status: &Arc<Mutex<MicStatus>>, is_playing: bool) {
    if let Ok(mut s) = status.lock() {
        s.is_playing = is_playing;
    }
}

// ── 设备枚举 ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioDevice {
    pub name: String,
    pub is_virtual_cable: bool,
    pub is_default: bool,
}

pub fn list_output_devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());
    host.output_devices()
        .map(|iter| {
            iter.filter_map(|d| {
                let name = d.name().ok()?;
                Some(AudioDevice {
                    is_virtual_cable: is_vb_cable(&name),
                    is_default: default_name.as_deref() == Some(name.as_str()),
                    name,
                })
            })
            .collect()
        })
        .unwrap_or_default()
}

fn is_vb_cable(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("cable") || lower.contains("vb-audio") || lower.contains("vb audio")
}

// ── Tauri 命令 ──

/// 枚举可用输出设备（含 VB-CABLE 标记）
#[tauri::command]
pub fn list_mic_devices() -> Vec<AudioDevice> {
    list_output_devices()
}

/// 检测是否安装了 VB-CABLE 虚拟声卡
#[tauri::command]
pub fn check_vb_cable() -> bool {
    list_output_devices().iter().any(|d| d.is_virtual_cable)
}

/// 手动播放音频到指定设备（audio_path 为绝对路径）
#[tauri::command]
pub fn play_to_mic(
    state: tauri::State<'_, MicPlayback>,
    audio_path: String,
    device_name: String,
    volume: Option<f32>,
) {
    state.play(PathBuf::from(audio_path), device_name, volume.unwrap_or(1.0));
}

/// 停止虚拟麦克风播放
#[tauri::command]
pub fn stop_mic(state: tauri::State<'_, MicPlayback>) {
    state.stop();
}

/// 获取播放状态
#[tauri::command]
pub fn get_mic_status(state: tauri::State<'_, MicPlayback>) -> MicStatus {
    state.status()
}