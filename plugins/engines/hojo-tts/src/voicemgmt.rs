// 音色管理：单个音色的安装 / 卸载 / 预加载 / 自定义音色包导入。
//
// 与 setup.rs 的分工：setup 负责"整体环境"（Python 运行时 + 模型 + 默认音色），
// 本模块负责"音色级"操作。安装复用 setup::ensure_all 链路（环境未就绪会先补
// 环境，进度文案如实报告）；预加载只走幂等的补齐链路、绝不触发下载；
// 卸载/导入是纯磁盘操作。全部操作经 ctx.ensure_lock 串行，与合成入口互斥。

use std::path::Path;

use crate::paths::Ctx;
use crate::voices;

/// 确保音色包落盘（幂等）：已装直接返回；预置角色缺失则从上游下载
/// （仅显式安装流程调用——run_setup / install_voice，均有进度可见）；
/// 用户自备音色缺失则报错引导导入。
pub fn ensure_voice_pack(ctx: &Ctx, voice_id: &str) -> Result<(), String> {
    if !voices::is_valid_pack_id(voice_id) {
        return Err(format!("音色 id「{voice_id}」不合法"));
    }
    let pack_dir = ctx.voices_dir().join(voice_id);
    if pack_ready(&pack_dir) {
        return Ok(());
    }
    let pre = voices::predefined(voice_id).ok_or_else(|| {
        format!(
            "音色「{}」尚未导入：自备音色请到 设置 → 音色管理 中导入音色包目录",
            voices::display_label(voice_id)
        )
    })?;
    download_predefined(ctx, pre.id, pre.asset, pre.text, pre.text_source)
}

/// 音色包布局是否完整（与 voices.rs 扫描口径一致）
fn pack_ready(pack_dir: &Path) -> bool {
    pack_dir.join("ref.wav").is_file() && pack_dir.join("voice.json").is_file()
}

/// 下载预置角色：参考音频（多源回退）→ voice.json 最后落盘（防半包被扫描）
fn download_predefined(
    ctx: &Ctx,
    id: &str,
    asset: &str,
    text: &str,
    text_source: &str,
) -> Result<(), String> {
    let label = voices::display_label(id);
    let pack_dir = ctx.voices_dir().join(id);
    let tmp_dir = ctx.dl_dir().join(id);
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("创建下载目录失败: {e}"))?;
    let tmp_wav = tmp_dir.join("ref.wav");

    let urls = voices::predefined_asset_urls(asset);
    let url_refs: Vec<&str> = urls.iter().map(|s| s.as_str()).collect();
    crate::client::download_with_fallback(&url_refs, &tmp_wav, None)
        .map_err(|e| format!("下载音色「{label}」参考音频失败: {e}"))?;

    std::fs::create_dir_all(&pack_dir)
        .map_err(|e| format!("创建音色目录失败: {e}"))?;
    std::fs::rename(&tmp_wav, pack_dir.join("ref.wav"))
        .map_err(|e| format!("落盘参考音频失败: {e}"))?;
    // voice.json 最后写：音色表扫描以 ref.wav + voice.json 齐全为准
    let meta = serde_json::json!({
        "label": label,
        "text": text,
        "text_source": text_source,
        "predefined": true,
    });
    std::fs::write(
        pack_dir.join("voice.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    )
    .map_err(|e| format!("写入音色信息失败: {e}"))?;
    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
}

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

/// 预加载已安装音色到内存（构造推理引擎，首次较慢）。
/// 未安装的音色直接报错，绝不触发下载。
pub fn preload_voice(voice_id: &str) -> Result<String, String> {
    if !voices::is_valid_pack_id(voice_id) {
        return Err(format!("音色 id「{voice_id}」不合法"));
    }
    if !voices::installed_pack_ids().iter().any(|id| id == voice_id) {
        return Err(format!(
            "音色「{}」尚未安装，请先在音色管理中安装",
            voices::display_label(voice_id)
        ));
    }

    let ctx = Ctx::get()?;
    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    // 已安装 → ensure_all 各阶段全是幂等快路径（磁盘检查/探活/load_voice），
    // 不会触发下载
    let opts = crate::setup::SetupOptions {
        voice: Some(voice_id.to_string()),
    };
    crate::setup::ensure_all(ctx, &opts, None)?;
    Ok(format!("音色「{}」已加载", voices::display_label(voice_id)))
}

/// 卸载音色：删除音色包目录。
/// Hojo 的参考音频在每次合成时现编码，服务端没有按音色的内存状态需要释放
/// （推理引擎全局共享，随服务进程常驻）。
pub fn uninstall_voice(voice_id: &str) -> Result<String, String> {
    if !voices::is_valid_pack_id(voice_id) {
        return Err(format!("音色 id「{voice_id}」不合法"));
    }
    let ctx = Ctx::get()?;
    let label = voices::display_label(voice_id);
    let pack_dir = ctx.voices_dir().join(voice_id);
    if !pack_dir.is_dir() {
        return Err(format!("音色「{label}」未安装，无需卸载"));
    }

    let _guard = ctx
        .ensure_lock
        .lock()
        .map_err(|e| format!("插件内部锁异常: {e}"))?;

    std::fs::remove_dir_all(&pack_dir)
        .map_err(|e| format!("删除音色文件失败（可能被占用，请关闭应用后重试）: {e}"))?;
    Ok(format!("音色「{label}」已卸载"))
}

/// 导入用户自备音色包：校验布局 → 复制到 voices/<目录名>/（保留用户原文件）。
/// 复制顺序刻意把 voice.json 放最后：音色表扫描以"ref.wav + voice.json
/// 齐全"为准，这样复制完成前不会被扫描成半个包。
///
/// 自备音色包目录布局（克隆自己的声音：一段 5~10 秒干净人声 + 逐字文本）：
///   <目录名即音色id>/
///     ├── ref.wav     # 参考音频
///     └── voice.json  # {"label": "展示名", "text": "参考音频的逐字文本"}
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
    let dest = ctx.voices_dir().join(&id);
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

    copy_pack(src, &dest).map_err(|e| {
        // 复制失败清掉残留，避免留下半个包
        let _ = std::fs::remove_dir_all(&dest);
        format!("复制音色包失败: {e}")
    })?;

    // 落盘后再校验一次（防御性）
    voices::validate_pack_layout(&dest)?;
    Ok(format!("音色「{}」导入成功", voices::display_label(&id)))
}

/// 复制音色包（ref.wav 先行，voice.json 收尾；多余文件一并带上）
fn copy_pack(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建目录失败: {e}"))?;

    let mut voice_json = None;
    let entries = std::fs::read_dir(src).map_err(|e| format!("读取目录失败: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let from = entry.path();
        let to = dest.join(&name);
        if name == "voice.json" && from.is_file() {
            voice_json = Some((from, to));
            continue;
        }
        if from.is_dir() {
            copy_pack(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("复制 {} 失败: {e}", from.display()))?;
        }
    }
    if let Some((from, to)) = voice_json {
        std::fs::copy(&from, &to)
            .map_err(|e| format!("复制 {} 失败: {e}", from.display()))?;
    }
    Ok(())
}
