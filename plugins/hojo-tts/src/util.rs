// 通用小工具：zip 解压等。

use std::path::Path;

/// 解压 zip 到目标目录（保留子目录结构；拒绝路径穿越条目）
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("打开压缩包失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("压缩包损坏: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩包条目失败: {e}"))?;
        // enclosed_name 拒绝 ../ 与绝对路径（防穿越）
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let out = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)
                .map_err(|e| format!("创建目录失败: {e}"))?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败: {e}"))?;
            }
            let mut f = std::fs::File::create(&out)
                .map_err(|e| format!("写入文件失败: {e}"))?;
            std::io::copy(&mut entry, &mut f)
                .map_err(|e| format!("解压文件失败: {e}"))?;
        }
    }
    Ok(())
}
