// 音频引用判断 + "无引用则删"清理。
//
// 用户拍板的原则：一条音频对应的来源消息已被删除、且没有任何收藏直接引用它，
// 才删该音频文件；只要二者之一还在引用，音频就保留。
//
// 之所以要扫两份 JSON，是因为同一个音频路径可能同时出现在
// messages.json（作为消息产物）和 favorites.json（作为收藏导入的音频）。
// 两个文件都查一遍才能确定"真的没人用了"。

use std::path::Path;
use super::atomic::Result;
use super::messages;
use super::favorites;

/// 判断某条音频路径是否还被引用（出现在 messages 或 favorites 中）。
///
/// `audio_path` 是相对 app_data_dir 的路径，如 "audio/m_xxx.mp3"。
pub fn is_audio_referenced(data_dir: &Path, audio_path: &str) -> bool {
    // messages 里有条目的 audio_path == 它
    if messages::load_messages(data_dir)
        .iter()
        .any(|m| m.audio_path == audio_path)
    {
        return true;
    }
    // favorites 里有条目的 audio_path == 它
    if favorites::load_favorites(data_dir)
        .iter()
        .any(|f| f.audio_path == audio_path)
    {
        return true;
    }
    false
}

/// 若 `audio_path` 不再被任何消息/收藏引用，则删除磁盘上的音频文件。
///
/// - 文件不存在视为已删（忽略不报错）
/// - 仍被引用则保留，什么也不做
/// - 返回是否真的删了文件
pub fn maybe_delete_audio(data_dir: &Path, audio_path: &str) -> Result<bool> {
    if is_audio_referenced(data_dir, audio_path) {
        return Ok(false);
    }
    let abs = data_dir.join(audio_path);
    match std::fs::remove_file(&abs) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{Message, Favorite, gen_id, now_iso};
    use tempfile::tempdir;

    /// 在 data_dir/audio/ 下造一个假音频文件，返回其相对路径
    fn make_fake_audio(data_dir: &Path, name: &str) -> String {
        let rel = format!("audio/{name}.mp3");
        let abs = data_dir.join(&rel);
        std::fs::create_dir_all(data_dir.join("audio")).unwrap();
        std::fs::write(&abs, b"fake audio bytes").unwrap();
        assert!(abs.exists(), "测试前置：假音频必须存在");
        rel
    }

    fn msg(audio_path: String) -> Message {
        Message {
            id: gen_id("m"),
            content: "hi".into(),
            audio_path,
            created_at: now_iso(),
        }
    }

    fn fav(audio_path: String) -> Favorite {
        Favorite {
            id: gen_id("f"),
            source_message_id: None,
            note: "n".into(),
            audio_path,
            created_at: now_iso(),
            hotkey: None,
        }
    }

    #[test]
    fn 无引用则删() {
        let dir = tempdir().unwrap();
        let rel = make_fake_audio(&dir.path().to_path_buf(), "a");
        let deleted = maybe_delete_audio(&dir.path().to_path_buf(), &rel).unwrap();
        assert!(deleted, "无引用应删除");
        assert!(!dir.path().join(&rel).exists(), "文件应已删除");
    }

    #[test]
    fn 被消息引用则保留() {
        let dir = tempdir().unwrap();
        let rel = make_fake_audio(&dir.path().to_path_buf(), "b");
        messages::add_message(&dir.path().to_path_buf(), msg(rel.clone())).unwrap();
        let deleted = maybe_delete_audio(&dir.path().to_path_buf(), &rel).unwrap();
        assert!(!deleted, "被消息引用应保留");
        assert!(dir.path().join(&rel).exists());
    }

    #[test]
    fn 被收藏引用则保留() {
        let dir = tempdir().unwrap();
        let rel = make_fake_audio(&dir.path().to_path_buf(), "c");
        favorites::add_favorite(&dir.path().to_path_buf(), fav(rel.clone())).unwrap();
        let deleted = maybe_delete_audio(&dir.path().to_path_buf(), &rel).unwrap();
        assert!(!deleted, "被收藏引用应保留");
        assert!(dir.path().join(&rel).exists());
    }

    #[test]
    fn 文件不存在不报错() {
        let dir = tempdir().unwrap();
        let rel = "audio/never_existed.mp3";
        let deleted = maybe_delete_audio(&dir.path().to_path_buf(), rel).unwrap();
        assert!(!deleted, "文件不存在视为已删，返回 false");
    }
}