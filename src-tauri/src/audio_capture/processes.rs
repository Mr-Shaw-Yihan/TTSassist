// 枚举「当前正在发声」的进程（供管理页下拉选择监听目标）。
//
// 原理：WASAPI 的 IAudioSessionManager2 能列出默认渲染设备上的所有音频会话，
// 每个会话（IAudioSessionControl2）可拿到归属进程 PID、会话显示名、活动状态。
// 我们按 PID 去重聚合（一个进程可能有多个会话），得到「图标+友好名（进程名）」候选。

use std::collections::BTreeMap;

use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::{
    eConsole, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::{AudioProcess, ComGuard};

/// 读取某 PID 的可执行文件名（如 "Discord.exe"）；失败返回 None。
fn process_exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 4096];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0), // 0 = _WIN32 路径
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..size as usize]);
        // 取路径末段为 exe 名
        let name = full
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(full.as_str())
            .to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
}

/// 从会话显示名（常为完整 exe 路径或应用名）取一个简短展示名。
fn short_display(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains('\\') || trimmed.contains('/') {
        let last = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
        let stem = last.strip_suffix(".exe").unwrap_or(last);
        if stem.is_empty() {
            None
        } else {
            Some(stem.to_string())
        }
    } else {
        Some(trimmed.to_string())
    }
}

/// 列出正在发声的进程音频会话（按 PID 聚合）。
///
/// 返回按显示名排序的列表；出错时返回 Err（前端提示"无法枚举音频会话"）。
pub fn list_active_processes() -> Result<Vec<AudioProcess>, String> {
    let _com = ComGuard::new_mta();

    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("创建设备枚举器失败：{e}"))?;

        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("无默认播放设备：{e}"))?;

        let manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("激活会话管理器失败：{e}"))?;

        let session_enum = manager
            .GetSessionEnumerator()
            .map_err(|e| format!("获取会话枚举器失败：{e}"))?;

        let count = session_enum
            .GetCount()
            .map_err(|e| format!("会话计数失败：{e}"))?;

        // pid -> (显示名, 进程名, 是否任一会话处于 Active)
        let mut agg: BTreeMap<u32, (Option<String>, Option<String>, bool)> = BTreeMap::new();

        for i in 0..count {
            let control = match session_enum.GetSession(i) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let c2: IAudioSessionControl2 = match control.cast() {
                Ok(c) => c,
                Err(_) => continue,
            };
            let pid = match c2.GetProcessId() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if pid == 0 {
                continue; // 系统空闲/无归属会话
            }
            let active = matches!(c2.GetState(), Ok(s) if s == AudioSessionStateActive);

            // 会话显示名（PWSTR，用完 CoTaskMemFree）
            let raw_display = match c2.GetDisplayName() {
                Ok(pw) => {
                    let s = pw.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(pw.0 as *const _));
                    s
                }
                Err(_) => String::new(),
            };

            let entry = agg.entry(pid).or_default();
            entry.2 |= active;
            if entry.0.is_none() {
                entry.0 = short_display(&raw_display);
            }
            if entry.1.is_none() {
                entry.1 = process_exe_name(pid);
            }
        }

        let mut out: Vec<AudioProcess> = agg
            .into_iter()
            .map(|(pid, (display, exe, active))| {
                let name = exe.clone().unwrap_or_else(|| format!("pid-{pid}"));
                let display_name = display.or(exe).unwrap_or_else(|| name.clone());
                AudioProcess {
                    pid,
                    name,
                    display_name,
                    is_active: active,
                }
            })
            .collect();

        // 活跃的排前面，其次按显示名
        out.sort_by(|a, b| {
            b.is_active
                .cmp(&a.is_active)
                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
        });
        Ok(out)
    }
}
