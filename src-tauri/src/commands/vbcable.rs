// VB-CABLE 驱动下载与安装命令。
//
// 功能：从 GitHub 下载 VB-CABLE 驱动包，解压后以管理员权限启动安装程序。
// 下载过程通过 Tauri 事件向前端发送进度更新。

use std::path::PathBuf;
use tauri::{Emitter, Manager};

/// GitHub  releases 下载 URL（与项目仓库中 VBCABLE_Driver_Pack45.zip 对应）
const VBCABLE_DOWNLOAD_URL: &str =
    "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v1.1.0/VBCABLE_Driver_Pack45.zip";

/// 下载进度事件名（前端 listen 该事件获取进度）
pub const VBCABLE_PROGRESS_EVENT: &str = "vbcable:download-progress";

/// 下载进度事件载荷
#[derive(Clone, serde::Serialize)]
struct DownloadProgress {
    /// "downloading" | "extracting" | "launching" | "done" | "error"
    stage: String,
    /// 已下载字节数
    downloaded: u64,
    /// 总字节数（从 Content-Length 获取，可能为 0）
    total: u64,
    /// 错误信息（仅 stage="error" 时有值）
    error: Option<String>,
}

/// 下载 VB-CABLE 驱动包到应用数据目录。
///
/// 下载过程通过 `vbcable:download-progress` 事件实时推送进度。
/// 返回下载的 zip 文件绝对路径。
#[tauri::command]
pub async fn download_vb_cable(app: tauri::AppHandle) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let download_dir = data_dir.join("downloads");
    std::fs::create_dir_all(&download_dir)
        .map_err(|e| format!("创建下载目录失败: {e}"))?;
    let zip_path = download_dir.join("VBCABLE_Driver_Pack45.zip");

    // 如果已下载过且文件完整（>1MB），跳过重复下载
    if zip_path.exists() {
        let meta = std::fs::metadata(&zip_path).ok();
        if meta.as_ref().map_or(false, |m| m.len() > 1_000_000) {
            let _ = app.emit(VBCABLE_PROGRESS_EVENT, DownloadProgress {
                stage: "done".into(),
                downloaded: meta.as_ref().unwrap().len(),
                total: meta.unwrap().len(),
                error: None,
            });
            return Ok(zip_path.to_string_lossy().to_string());
        }
    }

    // 发起 HTTP 请求
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let response = client
        .get(VBCABLE_DOWNLOAD_URL)
        .send()
        .await
        .map_err(|e| {
            let msg = if e.is_timeout() {
                "下载超时，请检查网络连接。可尝试手动下载：https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v1.1.0/VBCABLE_Driver_Pack45.zip"
            } else if e.is_connect() {
                "无法连接到 GitHub，请检查网络或代理设置。"
            } else {
                "下载失败"
            };
            format!("{msg}: {e}")
        })?;

    if !response.status().is_success() {
        return Err(format!("下载失败，HTTP 状态码: {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(&zip_path)
            .map_err(|e| format!("创建文件失败: {e}"))?,
    );

    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    let mut last_emit: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|e| format!("写入文件失败: {e}"))?;
        downloaded += chunk.len() as u64;

        // 每 50KB 发一次进度事件，避免事件风暴
        if downloaded - last_emit >= 50_000 || downloaded == total {
            let _ = app.emit(VBCABLE_PROGRESS_EVENT, DownloadProgress {
                stage: "downloading".into(),
                downloaded,
                total,
                error: None,
            });
            last_emit = downloaded;
        }
    }
    drop(file);

    let _ = app.emit(VBCABLE_PROGRESS_EVENT, DownloadProgress {
        stage: "done".into(),
        downloaded,
        total,
        error: None,
    });

    Ok(zip_path.to_string_lossy().to_string())
}

/// 解压已下载的 VB-CABLE 驱动包并以管理员权限启动安装程序。
///
/// 流程：解压 zip → 找到 VBCABLE_Setup_x64.exe → 以管理员身份运行。
/// 用户需在弹出的 UAC 对话框中确认，然后按安装向导完成安装。
#[tauri::command]
pub async fn install_vb_cable(zip_path: String) -> Result<String, String> {
    let zip_path = PathBuf::from(&zip_path);
    if !zip_path.exists() {
        return Err("驱动包不存在，请先下载".into());
    }

    // 解压到临时目录
    let extract_dir = tempfile::tempdir()
        .map_err(|e| format!("创建临时目录失败: {e}"))?;

    let file = std::fs::File::open(&zip_path)
        .map_err(|e| format!("打开 zip 失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("解压 zip 失败: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();

        // 安全检查：拒绝路径穿越
        if name.contains("..") {
            continue;
        }

        let out_path = extract_dir.path().join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).ok();
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut out = std::fs::File::create(&out_path)
                .map_err(|e| format!("创建文件失败: {e}"))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("写入文件失败: {e}"))?;
        }
    }

    // 查找安装程序（优先 x64）
    let setup = extract_dir.path().join("VBCABLE_Setup_x64.exe");
    let setup = if setup.exists() {
        setup
    } else {
        extract_dir.path().join("VBCABLE_Setup.exe")
    };

    if !setup.exists() {
        return Err("在压缩包中找不到安装程序".into());
    }

    // 以管理员权限启动安装程序（弹出 UAC 对话框）
    let setup_str = setup.to_string_lossy().replace('/', "\\");
    let ps_cmd = format!(
        "Start-Process -FilePath '{}' -Verb RunAs",
        setup_str
    );

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .output()
        .map_err(|e| format!("启动安装程序失败: {e}"))?;

    if output.status.success() {
        Ok("已启动 VB-CABLE 安装程序，请按向导完成安装后重启电脑。".into())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("was canceled") || stderr.contains("取消") {
            Err("用户取消了管理员权限授权。安装需要管理员权限。".into())
        } else {
            Err(format!("启动安装程序失败: {}", stderr.trim()))
        }
    }
}
