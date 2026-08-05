// 插件安装：zip 解压 + SHA-256 校验（不涉及 plugins 目录落盘，落盘由 PluginManager 负责）。
//
// 安全设计：
// - 在线安装：zip 先对照官方索引 checksum 校验，再解压
// - zip 只认平铺结构（manifest.json + dll），拒绝带路径的条目（防路径穿越）
// - enclosed_name 再拦一道 ../ 与绝对路径
// - 单文件/总量大小上限（防 zip 炸弹）
// - dll 对照 manifest.checksum 二次校验（防 zip 内被换包）

use std::collections::HashMap;
use std::path::Path;

use super::loader::sha256_file;
use super::manifest::PluginManifest;
use super::PluginError;

/// 单文件大小上限（插件 dll 一般几 MB）
const MAX_FILE_SIZE: u64 = 150 * 1024 * 1024;
/// 解压总量上限
const MAX_TOTAL_SIZE: u64 = 300 * 1024 * 1024;

/// 解压 + 校验完成的插件：临时目录（含 manifest.json 与 dll）+ 清单
pub struct StagedPlugin {
    pub dir: tempfile::TempDir,
    pub manifest: PluginManifest,
}

/// 解压插件 zip 并完成全部校验。
///
/// `expected_zip_checksum`：在线安装时传索引中的 zip SHA-256；拖入安装传 None
/// （拖入没有可信的 zip 校验源，靠 manifest.checksum 保证 dll 完整性，来源风险由 UI 提示）。
pub fn extract_and_verify(
    zip_path: &Path,
    expected_zip_checksum: Option<&str>,
) -> Result<StagedPlugin, PluginError> {
    // 1. zip 整体 SHA-256（在线安装）
    if let Some(expected) = expected_zip_checksum {
        let actual = sha256_file(zip_path)?;
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            return Err(PluginError::Checksum {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    // 2. 解压到内存（平铺结构限制 + 大小限制）
    let file = std::fs::File::open(zip_path)
        .map_err(|e| PluginError::Io(format!("打开 zip 失败: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| PluginError::Manifest(format!("zip 不是合法的压缩包: {e}")))?;

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| PluginError::Manifest(format!("读取 zip 条目失败: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        // enclosed_name 拒绝 ../ 与绝对路径；再要求平铺（无子目录）
        let name = entry
            .enclosed_name()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|n| !n.contains('/') && !n.contains('\\'))
            .ok_or_else(|| {
                PluginError::Manifest("zip 内含非法路径条目（只允许平铺的 manifest.json + dll）".into())
            })?;

        let size = entry.size();
        if size > MAX_FILE_SIZE {
            return Err(PluginError::Manifest(format!(
                "zip 内文件 {name} 过大（超过 150MB 上限）"
            )));
        }
        total += size;
        if total > MAX_TOTAL_SIZE {
            return Err(PluginError::Manifest("zip 内容总量过大（超过 300MB 上限）".into()));
        }

        let mut buf = Vec::with_capacity(size.min(MAX_FILE_SIZE) as usize);
        std::io::copy(&mut entry, &mut buf)
            .map_err(|e| PluginError::Io(format!("解压 {name} 失败: {e}")))?;
        files.insert(name, buf);
    }

    // 3. 解析并校验 manifest
    let manifest_raw = files
        .remove("manifest.json")
        .ok_or_else(|| PluginError::Manifest("zip 内缺少 manifest.json".into()))?;
    let manifest_str = std::str::from_utf8(&manifest_raw)
        .map_err(|_| PluginError::Manifest("manifest.json 不是有效的 UTF-8".into()))?;
    let manifest: PluginManifest = serde_json::from_str(manifest_str.trim_start_matches('\u{FEFF}'))
        .map_err(|e| PluginError::Manifest(format!("manifest.json 解析失败: {e}")))?;
    manifest.validate(super::manager::APP_VERSION)?;

    // 4. dll 对照 manifest.checksum 校验
    let dll = files
        .remove(&manifest.entry)
        .ok_or_else(|| PluginError::Manifest(format!("zip 内缺少 {}", manifest.entry)))?;
    let actual = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&dll);
        format!("{:x}", hasher.finalize())
    };
    if !actual.eq_ignore_ascii_case(manifest.checksum.trim()) {
        return Err(PluginError::Checksum {
            expected: manifest.checksum.clone(),
            actual,
        });
    }

    // 5. 落到临时目录（供调用方搬移）
    let dir = tempfile::TempDir::new()
        .map_err(|e| PluginError::Io(format!("创建临时目录失败: {e}")))?;
    std::fs::write(dir.path().join("manifest.json"), &manifest_raw)
        .map_err(|e| PluginError::Io(format!("写入临时清单失败: {e}")))?;
    std::fs::write(dir.path().join(&manifest.entry), &dll)
        .map_err(|e| PluginError::Io(format!("写入临时 dll 失败: {e}")))?;

    Ok(StagedPlugin { dir, manifest })
}
