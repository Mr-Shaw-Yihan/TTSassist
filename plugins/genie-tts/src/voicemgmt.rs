// 音色管理（阶段 21）：单个音色的安装 / 卸载 / 预加载 / 自定义音色包导入。
//
// 与 setup.rs 的分工：setup 负责"整体环境"（Python 运行时 + 语音资源 + 默认音色），
// 本模块负责"音色级"操作。安装复用 setup::ensure_all 链路（环境未就绪会先补环境，
// 进度文案如实报告）；预加载只走幂等的补齐链路、绝不触发下载；卸载/导入是纯磁盘操作。
// 全部操作经 ctx.ensure_lock 串行，与合成入口互斥。

use std::path::Path;

use crate::paths::Ctx;
use crate::voices;

/// 安装指定音色（幂等）：环境未就绪会先补环境（进度文案如实报告当前阶段），
/// 然后下载/加载音色。进度分段沿用 ensure_all 的划分。
pub fn install_voice(voice_id: &str, cb: &dyn Fn(f32, &str)) -> Result<String, String> {
    if !voices::is_valid_pack_id(voice_id) {
        return Err(format!("音色 id「{voice_id}」不合法"));
    }
    let label = voices::display_label(voice_id);

    let ctx = Ctx::get()?;
    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    let opts = crate::setup::SetupOptions {
        voice: Some(voice_id.to_string()),
    };
    crate::setup::ensure_all(ctx, &opts, Some(cb))?;
    cb(100.0, "安装完成");
    Ok(format!("音色「{label}」已就绪，现在可以使用了"))
}

/// 预加载已安装音色到内存（加载模型权重，秒级）。
/// 未安装的音色直接报错，绝不触发下载。
pub fn preload_voice(voice_id: &str) -> Result<String, String> {
    if !voices::is_valid_pack_id(voice_id) {
        return Err(format!("音色 id「{voice_id}」不合法"));
    }
    if !voices::installed_pack_ids().iter().any(|id| id == voice_id) {
        return Err(format!(
            "音色「{}」尚未安装，请先在音色管理中下载",
            voices::display_label(voice_id)
        ));
    }

    let ctx = Ctx::get()?;
    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    // 已安装 → ensure_all 各阶段全是幂等快路径（磁盘检查/探活/load_character），
    // 不会触发下载
    let opts = crate::setup::SetupOptions {
        voice: Some(voice_id.to_string()),
    };
    crate::setup::ensure_all(ctx, &opts, None)?;
    Ok(format!("音色「{}」已加载", voices::display_label(voice_id)))
}

/// 卸载音色：先让服务端释放内存（若服务在跑），再删除音色包目录。
pub fn uninstall_voice(voice_id: &str) -> Result<String, String> {
    if !voices::is_valid_pack_id(voice_id) {
        return Err(format!("音色 id「{voice_id}」不合法"));
    }
    let ctx = Ctx::get()?;
    let label = voices::display_label(voice_id);
    let pack_dir = ctx.characters_dir().join(voice_id);
    if !pack_dir.is_dir() {
        return Err(format!("音色「{label}」未安装，无需卸载"));
    }

    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    // 服务在跑就先卸载内存中的权重（尽力而为：失败不阻塞文件删除）
    if let Some(port) = crate::server::running_port() {
        if let Err(e) = crate::client::unload_character(port, voice_id) {
            eprintln!("[genie-tts] 卸载内存音色失败（不影响删文件）: {e}");
        }
    }

    std::fs::remove_dir_all(&pack_dir)
        .map_err(|e| format!("删除音色文件失败（可能被占用，请关闭应用后重试）: {e}"))?;
    Ok(format!("音色「{label}」已卸载"))
}

/// 导入用户自备音色包：校验布局 → 复制到 characters/<目录名>/（保留用户原文件）。
/// 复制顺序刻意把 prompt_wav.json 放最后：音色表扫描以"tts_models/ + prompt_wav.json
/// 齐全"为准，这样复制完成前不会被扫描成半个包。
pub fn import_voice_pack(src_dir: &str) -> Result<String, String> {
    let src = Path::new(src_dir);
    voices::validate_pack_layout(src)?;

    // 音色 id = 源目录名（不合法时提示用户改名，不擅自替他起名）
    let id = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| voices::is_valid_pack_id(n))
        .ok_or_else(|| {
            "音色包文件夹名只能包含字母、数字、下划线、连字符，请改名后重试".to_string()
        })?;

    let ctx = Ctx::get()?;
    let dest = ctx.characters_dir().join(&id);
    if dest.exists() {
        return Err(format!(
            "已存在同名音色「{}」，请先卸载后再导入（避免误覆盖）",
            voices::display_label(&id)
        ));
    }

    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    copy_dir_recursive(src, &dest).map_err(|e| {
        // 复制失败清掉残留，避免留下半个包
        let _ = std::fs::remove_dir_all(&dest);
        format!("复制音色包失败: {e}")
    })?;

    // 落盘后再校验一次（防御性）
    voices::validate_pack_layout(&dest)?;
    Ok(format!("音色「{}」导入成功", voices::display_label(&id)))
}

/// 递归复制目录（保留结构；失败即中断由调用方清理）
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建目录失败: {e}"))?;

    // 先复制除 prompt_wav.json 外的内容，最后复制它（见 import_voice_pack 注释）
    let mut prompt_wav_json = None;
    let entries = std::fs::read_dir(src).map_err(|e| format!("读取目录失败: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let from = entry.path();
        let to = dest.join(&name);
        if name == "prompt_wav.json" && from.is_file() {
            prompt_wav_json = Some((from, to));
            continue;
        }
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("复制 {} 失败: {e}", from.display()))?;
        }
    }
    if let Some((from, to)) = prompt_wav_json {
        std::fs::copy(&from, &to)
            .map_err(|e| format!("复制 {} 失败: {e}", from.display()))?;
    }
    Ok(())
}
