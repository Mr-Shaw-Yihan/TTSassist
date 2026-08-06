// 插件加载器：libloading 加载 .dll → 取 C ABI 符号 → 包装成 TTSEngine。
//
// 加载顺序（任一环节失败即弃用该插件，不影响主程序）：
// 1. 读 manifest.json 并校验（类型/平台/最低版本/id）
// 2. 按 manifest.checksum 对 dll 做 SHA-256 校验（防篡改）
// 3. libloading 加载 dll，取全部导出符号
// 4. 立即拷贝元信息（id/音色表/音频格式）——之后不再为元信息触碰 dll
// 5. 校验 dll 自报的 va_plugin_id == manifest.id（防换包）
//
// 卸载注意：插件内部可能有常驻线程（如 TTS 插件的异步运行时），
// 运行期卸载 dll 会导致崩溃，因此 LoadedPlugin 一经加载即常驻到进程退出。

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::manifest::PluginManifest;
use super::PluginError;
use crate::storage::types::gen_id;
use crate::tts::traits::{EngineCategory, TTSEngine, TTSParams, TtsError};

/// 一个已加载的插件（dll 句柄 + 函数指针 + 元信息缓存）
pub struct LoadedPlugin {
    /// 清单（含 id/name/version/description 等展示信息）
    pub manifest: PluginManifest,
    /// dll 自报的 id（加载时已校验与 manifest.id 一致）
    pub dll_id: String,
    /// dll 自报的音频格式扩展名（如 "mp3"/"wav"）
    pub audio_format: String,
    /// va_list_voices 返回的 JSON（加载时已拷贝；动态音色插件此为初值，
    /// 展示用最新音色表请调 query_voices_json）
    pub voices_json: String,
    /// 保持 dll 句柄存活（函数指针有效性依赖它）
    _lib: Arc<libloading::Library>,
    f_synthesize: plugin_api::VaTtsSynthesizeFn,
    f_list_voices: plugin_api::VaStrFn,
    f_free_bytes: plugin_api::VaFreeBytesFn,
    f_free_cstr: plugin_api::VaFreeCstrFn,
}

// libloading::Library 是 Send+Sync，函数指针天然 Send+Sync
unsafe impl Send for LoadedPlugin {}
unsafe impl Sync for LoadedPlugin {}

impl LoadedPlugin {
    /// 加载一个插件目录（内含 manifest.json 与 dll）
    pub fn load(plugin_dir: &Path, app_version: &str) -> Result<Arc<Self>, PluginError> {
        // 1. 读清单 + 校验
        let manifest = PluginManifest::load(plugin_dir)?;
        manifest.validate(app_version)?;

        // 1.5 为插件准备数据目录并通过环境变量告知（本地模型类插件在此放模型/缓存）。
        // 变量名按插件 id 区分（大写、连字符转下划线），多插件互不覆盖；
        // 加载是串行进行的，此处 set_var 无并发问题。插件应在初始化时读取并缓存。
        let data_dir = plugin_dir.join("data");
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            eprintln!("提示：插件「{}」数据目录创建失败: {e}", manifest.id);
        }
        let env_key = format!(
            "VA_PLUGIN_DATA_DIR_{}",
            manifest.id.to_ascii_uppercase().replace('-', "_")
        );
        std::env::set_var(&env_key, &data_dir);

        // 2. SHA-256 校验 dll
        let dll_path = plugin_dir.join(&manifest.entry);
        if !dll_path.exists() {
            return Err(PluginError::NotFound(format!(
                "插件动态库不存在: {}",
                dll_path.display()
            )));
        }
        let actual = sha256_file(&dll_path)?;
        if !actual.eq_ignore_ascii_case(manifest.checksum.trim()) {
            return Err(PluginError::Checksum {
                expected: manifest.checksum.clone(),
                actual,
            });
        }

        // 3. 加载 dll 取符号
        let lib = unsafe { libloading::Library::new(&dll_path) }
            .map_err(|e| PluginError::DlOpen(format!("加载 {} 失败: {e}", manifest.entry)))?;
        let (f_synthesize, f_free_bytes, f_free_cstr, f_list_voices, dll_id, name, version, audio_format, voices_json) = unsafe {
            let synthesize = get_sym::<plugin_api::VaTtsSynthesizeFn>(&lib, plugin_api::SYM_TTS_SYNTHESIZE)?;
            let free_bytes = get_sym::<plugin_api::VaFreeBytesFn>(&lib, plugin_api::SYM_FREE_BYTES)?;
            let free_cstr = get_sym::<plugin_api::VaFreeCstrFn>(&lib, plugin_api::SYM_FREE_CSTR)?;
            let f_id = get_sym::<plugin_api::VaStrFn>(&lib, plugin_api::SYM_PLUGIN_ID)?;
            let f_name = get_sym::<plugin_api::VaStrFn>(&lib, plugin_api::SYM_PLUGIN_NAME)?;
            let f_version = get_sym::<plugin_api::VaStrFn>(&lib, plugin_api::SYM_PLUGIN_VERSION)?;
            let f_format = get_sym::<plugin_api::VaStrFn>(&lib, plugin_api::SYM_AUDIO_FORMAT)?;
            let f_voices = get_sym::<plugin_api::VaStrFn>(&lib, plugin_api::SYM_LIST_VOICES)?;

            // 4. 立即拷贝元信息
            (
                synthesize,
                free_bytes,
                free_cstr,
                f_voices,
                read_cstr(f_id())?,
                read_cstr(f_name())?,
                read_cstr(f_version())?,
                read_cstr(f_format())?,
                read_cstr(f_voices())?,
            )
        };

        // 5. dll 自报 id 必须与清单一致；dll 自报版本仅记录（清单为准）
        if dll_id != manifest.id {
            return Err(PluginError::Unsupported(format!(
                "dll 自报 id「{dll_id}」与清单 id「{}」不一致，拒绝加载",
                manifest.id
            )));
        }
        if version != manifest.version {
            eprintln!(
                "提示：插件「{}」dll 版本 {version} 与清单版本 {} 不一致（以清单为准）",
                manifest.id, manifest.version
            );
        }
        let _ = name; // dll 自报名仅调试用，展示以清单为准

        Ok(Arc::new(Self {
            manifest,
            dll_id,
            audio_format,
            voices_json,
            _lib: Arc::new(lib),
            f_synthesize,
            f_list_voices,
            f_free_bytes,
            f_free_cstr,
        }))
    }

    /// 实时重查音色表（调 dll 的 va_list_voices 并立即拷贝）。
    /// 静态音色插件返回不变的字面量；动态音色插件（本地模型类）返回最新音色列表。
    /// 失败（dll 返回空指针等）时回退到加载时缓存的 voices_json。
    pub fn query_voices_json(&self) -> String {
        let ptr = unsafe { (self.f_list_voices)() };
        let copied = unsafe { read_cstr(ptr) };
        match copied {
            Ok(s) => s,
            Err(_) => self.voices_json.clone(),
        }
    }

    /// 安全封装的合成调用：文本(+音色) → 音频字节。
    /// 负责 FFI 入参转换、出参拷贝、插件内存归还。阻塞调用，需在 blocking 线程执行。
    pub fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<Vec<u8>, PluginError> {
        let c_text = CString::new(text)
            .map_err(|_| PluginError::Synthesize("文本含非法字符（NUL）".into()))?;
        let c_voice = match voice {
            Some(v) => Some(
                CString::new(v)
                    .map_err(|_| PluginError::Synthesize("音色名含非法字符（NUL）".into()))?,
            ),
            None => None,
        };

        let mut out_audio: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();

        let code = unsafe {
            (self.f_synthesize)(
                c_text.as_ptr(),
                c_voice.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null()),
                &mut out_audio,
                &mut out_len,
                &mut out_err,
            )
        };

        if code == plugin_api::VA_OK {
            if out_audio.is_null() {
                return Err(PluginError::Synthesize("插件返回成功但未给出音频".into()));
            }
            // 立即拷贝到宿主内存，然后归还插件的缓冲
            let bytes = unsafe { std::slice::from_raw_parts(out_audio, out_len) }.to_vec();
            unsafe { (self.f_free_bytes)(out_audio, out_len) };
            Ok(bytes)
        } else {
            let msg = if !out_err.is_null() {
                let s = unsafe { CStr::from_ptr(out_err) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { (self.f_free_cstr)(out_err) };
                s
            } else {
                format!("插件合成失败（错误码 {code}）")
            };
            Err(PluginError::Synthesize(msg))
        }
    }
}

/// 取符号并复制出函数指针
unsafe fn get_sym<T>(
    lib: &libloading::Library,
    name: &[u8],
) -> Result<T, PluginError>
where
    T: Copy,
{
    lib.get::<T>(name).map(|sym| *sym).map_err(|e| {
        // 去掉符号名末尾的 NUL 再展示
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        PluginError::Symbol(format!(
            "插件缺少导出函数 {}: {e}",
            String::from_utf8_lossy(&name[..end])
        ))
    })
}

/// 读取 C 字符串为 String（立即拷贝）
unsafe fn read_cstr(ptr: *const std::ffi::c_char) -> Result<String, PluginError> {
    if ptr.is_null() {
        return Err(PluginError::Symbol("插件返回了空字符串指针".into()));
    }
    Ok(CStr::from_ptr(ptr).to_string_lossy().into_owned())
}

/// 计算文件 SHA-256（流式，适配几 MB 的 dll）
pub fn sha256_file(path: &Path) -> Result<String, PluginError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|e| PluginError::Io(format!("打开 {} 失败: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| PluginError::Io(format!("读取 {} 失败: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 把已加载插件包装成 TTSEngine：合成 → 写 audio/ 目录 → 返回相对路径
pub struct PluginEngine {
    plugin: Arc<LoadedPlugin>,
    data_dir: PathBuf,
}

impl PluginEngine {
    pub fn new(plugin: Arc<LoadedPlugin>, data_dir: PathBuf) -> Self {
        Self { plugin, data_dir }
    }
}

#[async_trait::async_trait]
impl TTSEngine for PluginEngine {
    fn name(&self) -> &str {
        &self.plugin.manifest.id
    }

    fn category(&self) -> EngineCategory {
        // 按清单 category 字段区分本地/联网引擎（缺省 remote，向后兼容老插件）
        match self.plugin.manifest.category.as_str() {
            "local" => EngineCategory::Local,
            _ => EngineCategory::Remote,
        }
    }

    async fn generate(&self, params: TTSParams<'_>) -> Result<String, TtsError> {
        let plugin = Arc::clone(&self.plugin);
        let text = params.text.to_string();
        let voice = params.voice.map(str::to_string);
        let timeout_secs = self.plugin.manifest.timeout_secs;

        // FFI 合成是阻塞调用 → 丢到 blocking 线程池，避免卡住异步运行时。
        // 本地引擎首次推理可能加载模型（数十秒）甚至引导下载运行环境（数分钟），
        // 超时上限由清单 timeout_secs 声明（默认 60s）。
        let fut = tauri::async_runtime::spawn_blocking(move || {
            plugin.synthesize(&text, voice.as_deref())
        });
        let audio = match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            fut,
        )
        .await
        {
            Ok(join_result) => join_result
                .map_err(|e| TtsError::Network(format!("插件任务中断: {e}")))?
                .map_err(|e| TtsError::Network(e.to_string()))?,
            Err(_) => {
                return Err(TtsError::Network(format!(
                    "插件合成超时（超过 {timeout_secs} 秒）。本地引擎首次使用需下载运行环境与模型，请联网后重试；若反复超时请检查插件状态。"
                )));
            }
        };

        if audio.is_empty() {
            return Err(TtsError::Network("插件未返回音频".into()));
        }

        // 按插件声明的格式落盘（mp3/wav/...）
        let ext = self.plugin.audio_format.trim();
        let ext = if ext.is_empty() { "mp3" } else { ext };
        let id = gen_id("m");
        let rel_path = format!("audio/{id}.{ext}");
        let abs = self.data_dir.join(&rel_path);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TtsError::WriteFile(format!("创建目录失败: {e}")))?;
        }
        std::fs::write(&abs, &audio)
            .map_err(|e| TtsError::WriteFile(format!("写入音频失败: {e}")))?;

        Ok(rel_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_已知值() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"abc").unwrap();
        // "abc" 的 SHA-256 是固定值
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
