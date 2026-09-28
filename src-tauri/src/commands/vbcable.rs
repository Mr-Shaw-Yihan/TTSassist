// VB-CABLE 驱动下载与安装命令。
//
// 功能：从 GitHub 下载 VB-CABLE 驱动包，解压后以管理员权限启动安装程序。
// 下载过程通过 Tauri 事件向前端发送进度更新。

use std::path::PathBuf;
use tauri::{Emitter, Manager};

/// GitHub  releases 下载 URL（与项目仓库中 VBCABLE_Driver_Pack45.zip 对应）
const VBCABLE_DOWNLOAD_URL: &str =
    "https://github.com/Mr-Shaw-Yihan/TTSassist/releases/download/v1.1.0/VBCABLE_Driver_Pack45.zip";

/// 权威发布资源的 SHA-256。下载/复用缓存/提权安装前均校验，防中间人或 release 资源
/// 被替换后以管理员权限执行被篡改的驱动。真值经本地包与 v1.1.0 release 资源双端比对
/// 确认（大小 1,236,661 字节）。GitHub release 资源不可变，除非删重传，故此固定值稳定。
const VBCABLE_ZIP_SHA256: &str =
    "62c38a27e2bf0dd972c34e084c3d3f4e60bac9bdfc2cac36e750a772b01fea3e";

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

/// 计算文件 SHA-256（十六进制小写）。
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 校验文件 SHA-256 是否等于权威固定值（大小写不敏感）。
fn verify_pinned_sha256(path: &std::path::Path) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(VBCABLE_ZIP_SHA256) {
        Ok(())
    } else {
        Err(format!(
            "驱动包完整性校验失败：SHA-256 不匹配（期望 {VBCABLE_ZIP_SHA256}，实际 {actual}），已拒绝使用以防提权执行被篡改的程序"
        ))
    }
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

    // 如果已下载过且文件完整（>1MB），先校验 SHA-256 再决定是否复用缓存；
    // 命中缓存但校验不过（被篡改/损坏/投毒）则删除重下，绝不复用未验证的包。
    if zip_path.exists() {
        let meta = std::fs::metadata(&zip_path).ok();
        let big = meta.as_ref().map_or(false, |m| m.len() > 1_000_000);
        if big && verify_pinned_sha256(&zip_path).is_ok() {
            let len = meta.unwrap().len();
            let _ = app.emit(VBCABLE_PROGRESS_EVENT, DownloadProgress {
                stage: "done".into(),
                downloaded: len,
                total: len,
                error: None,
            });
            return Ok(zip_path.to_string_lossy().to_string());
        }
        if big {
            let _ = std::fs::remove_file(&zip_path);
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

    // 下载完成：校验 SHA-256，不符则删除并报错（拒绝把可疑包交给后续提权安装）
    if let Err(e) = verify_pinned_sha256(&zip_path) {
        let _ = std::fs::remove_file(&zip_path);
        let _ = app.emit(VBCABLE_PROGRESS_EVENT, DownloadProgress {
            stage: "error".into(),
            downloaded,
            total,
            error: Some(e.clone()),
        });
        return Err(e);
    }

    let _ = app.emit(VBCABLE_PROGRESS_EVENT, DownloadProgress {
        stage: "done".into(),
        downloaded,
        total,
        error: None,
    });

    Ok(zip_path.to_string_lossy().to_string())
}

/// 在解压目录中递归查找安装程序（优先 x64）。
///
/// 下载的 zip 内所有文件嵌套在顶层子目录 `VBCABLE_Driver_Pack45/` 下，
/// 因此不能只在解压根目录查找，需递归遍历整棵目录树。
fn find_setup_exe(dir: &std::path::Path) -> Option<PathBuf> {
    let mut x64: Option<PathBuf> = None;
    let mut plain: Option<PathBuf> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    match path.file_name().and_then(|n| n.to_str()) {
                        Some("VBCABLE_Setup_x64.exe") => {
                            if x64.is_none() {
                                x64 = Some(path);
                            }
                        }
                        Some("VBCABLE_Setup.exe") => {
                            if plain.is_none() {
                                plain = Some(path);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    x64.or(plain)
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

    // 纵深防御：解压/提权前再校验一次完整性（防止绕过 download 直接传入被替换的包）
    verify_pinned_sha256(&zip_path)?;

    // 解压到「持久」目录而非 tempfile::tempdir()：VBCABLE_Setup_x64.exe 不是自包含
    // 安装器，用户点"install"时才从自身所在目录加载 .inf/.sys/.cat。若用临时目录，
    // 本函数异步拉起安装器后立即返回、临时目录随之销毁，安装器届时找不到驱动文件，
    // 报 -106 LOADDRV: The path does not exist。
    let extract_root = zip_path
        .parent()
        .map(|p| p.join("VBCABLE_setup"))
        .unwrap_or_else(|| PathBuf::from("VBCABLE_setup"));
    if extract_root.exists() {
        let _ = std::fs::remove_dir_all(&extract_root);
    }
    std::fs::create_dir_all(&extract_root)
        .map_err(|e| format!("创建解压目录失败: {e}"))?;

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

        let out_path = extract_root.join(&name);
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

    // 查找安装程序（zip 内文件嵌套在顶层子目录，需递归查找，优先 x64）
    let setup = match find_setup_exe(&extract_root) {
        Some(s) => s,
        None => return Err("在压缩包中找不到安装程序".into()),
    };

    // 以管理员权限启动安装程序（弹出 UAC 对话框）。
    // 关键：显式设置 -WorkingDirectory 为安装程序所在目录，避免提权后进程 CWD 落到
    // System32 导致安装器按相对路径找不到同级驱动文件。
    let setup_str = setup.to_string_lossy().replace('/', "\\");
    let workdir = setup
        .parent()
        .map(|p| p.to_string_lossy().replace('/', "\\"))
        .unwrap_or_default();
    let ps_cmd = format!(
        "Start-Process -FilePath '{}' -WorkingDirectory '{}' -Verb RunAs",
        setup_str, workdir
    );

    let output = crate::proc::hidden_command("powershell")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 与 install_vb_cable 中一致的内联解压逻辑，供测试复用
    fn unzip_all(zip: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
        let f = std::fs::File::open(zip).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();
            if name.contains("..") {
                continue;
            }
            let out = dest.join(&name);
            if entry.is_dir() {
                std::fs::create_dir_all(&out).ok();
            } else {
                if let Some(p) = out.parent() {
                    std::fs::create_dir_all(p).ok();
                }
                let mut o = std::fs::File::create(&out).map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut o).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    #[test]
    fn find_setup_locates_nested_x64() {
        // 模拟 zip 内所有文件嵌套在顶层子目录的情况
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("VBCABLE_Driver_Pack45");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("VBCABLE_Setup.exe"), b"x").unwrap();
        std::fs::write(nested.join("VBCABLE_Setup_x64.exe"), b"x").unwrap();

        let found = find_setup_exe(dir.path()).expect("应能在嵌套子目录中找到安装程序");
        assert_eq!(found.file_name().unwrap(), "VBCABLE_Setup_x64.exe");
        assert_eq!(
            found.parent().unwrap().file_name().unwrap(),
            "VBCABLE_Driver_Pack45"
        );
    }

    #[test]
    fn find_setup_prefers_x64_and_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("VBCABLE_Setup.exe"), b"x").unwrap();
        // 只有 32 位时回退
        assert_eq!(
            find_setup_exe(dir.path()).unwrap().file_name().unwrap(),
            "VBCABLE_Setup.exe"
        );
        // 加入 x64 后优先选 x64
        std::fs::write(dir.path().join("VBCABLE_Setup_x64.exe"), b"x").unwrap();
        assert_eq!(
            find_setup_exe(dir.path()).unwrap().file_name().unwrap(),
            "VBCABLE_Setup_x64.exe"
        );
    }

    #[test]
    fn find_setup_returns_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"x").unwrap();
        assert!(find_setup_exe(dir.path()).is_none());
    }

    #[test]
    fn extract_real_zip_then_find_setup() {
        // 针对真实 VBCABLE_Driver_Pack45.zip 走一遍解压 + 定位（若本机存在该文件）
        let zip = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("VBCABLE_Driver_Pack45.zip");
        if !zip.exists() {
            eprintln!("跳过：未找到真实 zip {:?}", zip);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        unzip_all(&zip, dir.path()).unwrap();

        let found = find_setup_exe(dir.path()).expect("真实 zip 解压后应能定位安装程序");
        assert_eq!(found.file_name().unwrap(), "VBCABLE_Setup_x64.exe");
        assert_eq!(
            found.parent().unwrap().file_name().unwrap(),
            "VBCABLE_Driver_Pack45",
            "安装程序应位于嵌套子目录内，实际：{:?}",
            found
        );
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;

    #[test]
    fn pinned_sha256_is_well_formed() {
        // pin 应为 64 位小写 hex
        assert_eq!(VBCABLE_ZIP_SHA256.len(), 64);
        assert!(VBCABLE_ZIP_SHA256
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn sha256_file_matches_known_vector() {
        // 空输入的 SHA-256 固定值，验证实现无字节序/格式化错误
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("empty");
        std::fs::write(&f, b"").unwrap();
        assert_eq!(
            sha256_file(&f).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn verify_detects_tampered_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("evil.zip");
        std::fs::write(&f, b"not the real driver pack").unwrap();
        assert!(verify_pinned_sha256(&f).is_err(), "内容不符必须判失败");
    }

    #[test]
    fn verify_accepts_real_release_zip_when_present() {
        // 若本机存在真实 VBCABLE_Driver_Pack45.zip（仓库根），端到端验证 pin 与实际资源一致
        let zip = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("VBCABLE_Driver_Pack45.zip");
        if !zip.exists() {
            eprintln!("跳过：未找到真实 zip {:?}", zip);
            return;
        }
        verify_pinned_sha256(&zip).expect("真实 release zip 应通过 SHA-256 校验");
    }
}
