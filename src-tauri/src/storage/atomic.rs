// 原子写工具：先写临时文件再替换，防止"写到一半断电"损坏数据。
//
// 用户视角解释：直接覆盖写一个文件，如果中途断电/崩溃，文件就只剩半截内容、
// JSON 解析会失败、用户所有消息记录就废了。原子写的做法是先把完整新内容写到
// 一个临时文件，确认写好后，再用操作系统的"重命名"把它替换掉原文件——重命名
// 这一步在操作系统层面是原子的（要么成功、要么完全没发生），不会出现半截状态。
// 失败的最坏情况只是丢一个临时文件，原文件还完好。

use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("持久化（重命名）失败：从 {from} 到 {to}")]
    Persist { from: PathBuf, to: PathBuf },
    #[error("备注不能为空")]
    EmptyNote,
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// 把 `content` 原子地写入 `path`。
///
/// 会在 `path` 同目录下创建临时文件，写入内容后持久化（重命名）到 `path`。
/// 若 `path` 原本就存在，会被覆盖；不存在则创建。
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "路径无父目录"))?;
    // NamedTempFile 在 dir 内创建临时文件；persist 内部即原子的 rename。
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| StorageError::Persist {
        from: e.file.path().to_path_buf(),
        to: PathBuf::from(path),
    })?;
    Ok(())
}

/// 把一个可序列化对象写成漂亮 JSON 原子落盘。
pub fn write_json_pretty<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    atomic_write(path, json.as_bytes())
}

/// 读取文本文件；文件不存在返回 None（测试辅助用）。
pub fn load_text(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn 原子写_正常写入能读回() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.txt");
        atomic_write(&path, b"hello").unwrap();
        let got = std::fs::read_to_string(&path).unwrap();
        assert_eq!(got, "hello");
    }

    #[test]
    fn 原子写_覆盖已有文件() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.txt");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new content").unwrap();
        let got = std::fs::read_to_string(&path).unwrap();
        assert_eq!(got, "new content");
    }

    #[test]
    fn 原子写_无残留临时文件() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.txt");
        atomic_write(&path, b"x").unwrap();
        // 目录内只应有一个文件 a.txt（tempfile 持久化后临时文件被重命名而非保留）
        let entries = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn json_漂亮写入往返() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let val = serde_json::json!({"a":1,"b":[1,2]});
        write_json_pretty(&path, &val).unwrap();
        let got: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val, got);
    }
}