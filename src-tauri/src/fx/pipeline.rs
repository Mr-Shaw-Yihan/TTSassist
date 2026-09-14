// 效果器播放解析管线：resolve = 播放路径 →（效果开启时）效果缓存文件路径。
//
// 铁律（设计 §三/§十）：
// - 原文件永不修改；任何一步失败/关闭/非法 → fail-open 直通原文件，绝不阻断播放
// - 幂等：入参已在 .fx 缓存目录下时原样返回（后端 mic.play 与 playToMic 链都已 fx 化）
// - 指纹 = SHA-256("<plugin>:<effect>:<有序参数JSON>:<原文件字节长度>") 前 16 hex
// - 处理超时固定 10s：超时 warn + 直通 + 不重试 + 同（预设,原文件）本次进程内不再尝试
// - 缓存目录 200MB 按 mtime LRU 清理

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::codec;
use crate::plugins::loader::{LoadedFxPlugin, FxProcessedPcm};

/// 缓存目录（相对 data_dir）
pub const FX_CACHE_DIR: &str = "audio/.fx";
/// 「原声」固定保留值
pub const FX_PRESET_OFF: &str = "off";
/// 单次效果处理超时（固定 10s，不用 manifest.timeout_secs——用户不能干等）
const PROCESS_TIMEOUT_SECS: u64 = 10;
/// 缓存目录总量上限（超过即按 mtime 清最旧到目标值）
const CACHE_MAX_BYTES: u64 = 200 * 1024 * 1024;
const CACHE_TARGET_BYTES: u64 = 150 * 1024 * 1024;

/// 进程级超时/报错黑名单：键 = "<preset>\0<rel_path>"。插件处理是确定性的，
/// 同输入重试大概率同样失败/卡死，本次进程内不再尝试（fail-open 直通）。
fn blacklist() -> &'static Mutex<HashSet<String>> {
    static BLACKLIST: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    BLACKLIST.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 缓存指纹：预设全名 + 有序参数 + 原文件字节长度（防未来新增覆写路径与 gen_id 极端碰撞）
pub fn fingerprint(plugin_id: &str, effect_id: &str, params: &HashMap<String, String>, src_len: u64) -> String {
    use sha2::{Digest, Sha256};
    let ordered: BTreeMap<&String, &String> = params.iter().collect();
    let params_json = serde_json::to_string(&ordered).unwrap_or_else(|_| "{}".into());
    let material = format!("{plugin_id}:{effect_id}:{params_json}:{src_len}");
    let hash = Sha256::digest(material.as_bytes());
    hash.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// 效果解析入口（同步；阻塞耗时 = 缓存命中时仅哈希，未命中时一次解码+处理，
/// 内部自带 10s 超时保护线程）。永不失败：任何异常都直通原文件。
///
/// `plugin`：预设指向的效果器插件（调用方从 PluginManager::get_fx 取）；
/// None（插件未加载/已卸载）= 非法预设，静默直通。
pub fn resolve_with_plugin(
    data_dir: &Path,
    rel_path: &str,
    preset: &str,
    plugin: Option<Arc<LoadedFxPlugin>>,
    params_all: &HashMap<String, HashMap<String, String>>,
) -> PathBuf {
    let original_abs = data_dir.join(rel_path);

    // 幂等：已是效果缓存文件（后端 mic.play 与 playToMic 链传来的 abs 已 fx 化）
    if rel_path.starts_with(&format!("{FX_CACHE_DIR}/")) {
        return original_abs;
    }
    // 关闭/空值 = 原声直通
    if preset.is_empty() || preset == FX_PRESET_OFF {
        return original_abs;
    }
    // 全名必须是 "<plugin_id>:<effect_id>"，且插件已加载（非法预设 fail-open）
    let Some((plugin_id, effect_id)) = preset.split_once(':') else {
        return original_abs;
    };
    if plugin_id.is_empty() || effect_id.is_empty() {
        return original_abs;
    }
    let Some(plugin) = plugin else {
        return original_abs;
    };
    // 原文件必须存在（缺失时让既有播放错误路径去报）
    if !original_abs.is_file() {
        return original_abs;
    }

    let src_len = std::fs::metadata(&original_abs).map(|m| m.len()).unwrap_or(0);
    let params = params_all.get(preset).cloned().unwrap_or_default();
    let fp = fingerprint(plugin_id, effect_id, &params, src_len);

    let stem = original_abs
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio".into());
    let cache_dir = data_dir.join(FX_CACHE_DIR);
    let cache_abs = cache_dir.join(format!("{stem}.{fp}.wav"));
    if cache_abs.is_file() {
        return cache_abs;
    }

    // 黑名单：本次进程内该（预设, 原文件）已知失败/超时，不再尝试
    let bl_key = format!("{preset}\0{rel_path}");
    if blacklist().lock().map(|m| m.contains(&bl_key)).unwrap_or(true) {
        return original_abs;
    }

    match process_and_cache(&original_abs, &cache_abs, &cache_dir, plugin, effect_id, &params) {
        Ok(()) => cache_abs,
        Err(err) => {
            // 不含用户文本与绝对路径（隐私护栏）；err 内部只含错误类别
            log_warn!("效果处理失败，直通原声（preset={}）: {err}", preset);
            if let Ok(mut m) = blacklist().lock() {
                m.insert(bl_key);
            }
            original_abs
        }
    }
}

/// 处理并写缓存：解码 → FFI（10s 超时工作线程）→ 16-bit WAV → LRU 清理。
/// Err 内容为错误类别（无路径、无用户文本）。
fn process_and_cache(
    original_abs: &Path,
    cache_abs: &Path,
    cache_dir: &Path,
    plugin: Arc<LoadedFxPlugin>,
    effect_id: &str,
    params: &HashMap<String, String>,
) -> Result<(), String> {
    let (pcm, sample_rate, channels) = codec::decode_file_f32(original_abs)?;
    if pcm.is_empty() {
        return Err("解码结果为空".into());
    }

    let frames = pcm.len() / channels as usize;
    let params_json = serde_json::to_string(params).unwrap_or_else(|_| "{}".into());
    let effect_id = effect_id.to_string();
    let sample_rate_out = sample_rate;
    let cache_target = cache_abs.to_path_buf();
    let cache_dir_target = cache_dir.to_path_buf();

    // FFI 阻塞不可中断：丢独立线程跑（Arc 随线程存活），主流程 recv_timeout 保护；
    // 超时后线程跑完仍会把缓存写出（黑名单使本次进程内不再读它，下次启动直接命中）
    let (tx, rx) = std::sync::mpsc::channel::<Result<FxProcessedPcm, String>>();
    let pcm_handle = std::thread::Builder::new()
        .name("voiceassist-fx".into())
        .spawn(move || {
            let result = plugin
                .process_pcm(&pcm, frames, channels, sample_rate, &effect_id, &params_json)
                .map_err(|e| e.to_string());
            if let Ok(ref processed) = result {
                let _ = std::fs::create_dir_all(&cache_dir_target);
                let _ = codec::encode_wav16(
                    &cache_target,
                    &processed.pcm,
                    sample_rate_out,
                    processed.channels,
                );
                trim_cache(&cache_dir_target);
            }
            let _ = tx.send(result);
        })
        .map_err(|e| format!("效果处理线程启动失败: {e}"))?;

    match rx.recv_timeout(std::time::Duration::from_secs(PROCESS_TIMEOUT_SECS)) {
        Ok(Ok(_)) => {
            drop(pcm_handle);
            if !cache_abs.is_file() {
                return Err("插件返回但缓存未写出".into());
            }
            Ok(())
        }
        Ok(Err(e)) => Err(format!("插件处理失败: {e}")),
        Err(_) => Err(format!("效果处理超时（>{PROCESS_TIMEOUT_SECS}s）")),
    }
}

/// 缓存目录总量超上限时按 mtime 从旧到新删到目标值（简单 LRU）
fn trim_cache(cache_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            let mtime = meta.modified().ok()?;
            Some((mtime, meta.len(), p))
        })
        .collect();
    let total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= CACHE_MAX_BYTES {
        return;
    }
    files.sort_by_key(|(mtime, _, _)| *mtime);
    let mut remain = total;
    for (_, len, path) in files {
        if remain <= CACHE_TARGET_BYTES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            remain -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 指纹_参数顺序无关且长度参与() {
        let mut a = HashMap::new();
        a.insert("freq".into(), "55".into());
        a.insert("drive".into(), "2".into());
        let mut b = HashMap::new();
        b.insert("drive".into(), "2".into());
        b.insert("freq".into(), "55".into());
        assert_eq!(fingerprint("p", "e", &a, 100), fingerprint("p", "e", &b, 100));
        assert_ne!(fingerprint("p", "e", &a, 100), fingerprint("p", "e", &a, 200));
        assert_ne!(fingerprint("p", "e", &a, 100), fingerprint("p", "e2", &a, 100));
        assert_eq!(fingerprint("p", "e", &a, 100).len(), 16);
    }
}
