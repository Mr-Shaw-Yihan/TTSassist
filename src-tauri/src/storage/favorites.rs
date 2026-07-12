// favorites.json 读写
//
// 关键连带：
// - unlink_favorites_by_message：删消息时被调，把所有 source_message_id == id 的收藏置 None
// - delete_favorite：删收藏后，音频无引用则删（与 messages::delete_message 同原则）

use std::path::Path;
use super::atomic::{write_json_pretty, Result, StorageError};
use super::audio_gc::maybe_delete_audio;
use super::types::Favorite;

const FILE: &str = "favorites.json";

fn path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(FILE)
}

/// 读全部收藏。文件不存在返回空 Vec。
pub fn load_favorites(data_dir: &Path) -> Vec<Favorite> {
    let p = path(data_dir);
    if !p.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 添加一条收藏并原子写回。
/// note 不得为空（仅空白也算空），否则报错。
pub fn add_favorite(data_dir: &Path, favorite: Favorite) -> Result<()> {
    if favorite.note.trim().is_empty() {
        return Err(StorageError::EmptyNote);
    }
    let mut list = load_favorites(data_dir);
    list.push(favorite);
    write_json_pretty(&path(data_dir), &list)
}

/// 删除一条收藏：音频无引用则删。返回是否真的删了。
pub fn delete_favorite(data_dir: &Path, id: &str) -> Result<bool> {
    let list = load_favorites(data_dir);
    let target = list.iter().find(|f| f.id == id).cloned();
    let Some(target) = target else {
        return Ok(false);
    };

    let remaining: Vec<Favorite> = list.into_iter().filter(|f| f.id != id).collect();
    write_json_pretty(&path(data_dir), &remaining)?;

    // 先落数据再清文件
    maybe_delete_audio(data_dir, &target.audio_path)?;

    Ok(true)
}

/// 删消息时被调：把所有 source_message_id == message_id 的收藏置 None，原子写回。
pub fn unlink_favorites_by_message(data_dir: &Path, message_id: &str) -> Result<()> {
    let list = load_favorites(data_dir);
    if list.iter().all(|f| f.source_message_id.as_deref() != Some(message_id)) {
        // 没有需要改的，直接返回，避免无谓写入
        return Ok(());
    }
    let updated: Vec<Favorite> = list
        .into_iter()
        .map(|mut f| {
            if f.source_message_id.as_deref() == Some(message_id) {
                f.source_message_id = None;
            }
            f
        })
        .collect();
    write_json_pretty(&path(data_dir), &updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{gen_id, now_iso};
    use tempfile::tempdir;

    fn make_fav(note: &str, audio_path: &str) -> Favorite {
        Favorite {
            id: gen_id("f"),
            source_message_id: None,
            note: note.into(),
            audio_path: audio_path.into(),
            created_at: now_iso(),
        }
    }

    fn make_audio(dir: &std::path::Path, name: &str) -> String {
        let rel = format!("audio/{name}.mp3");
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        std::fs::write(dir.join(&rel), b"bytes").unwrap();
        rel
    }

    #[test]
    fn 文件不存在返回空() {
        let dir = tempdir().unwrap();
        assert!(load_favorites(&dir.path().to_path_buf()).is_empty());
    }

    #[test]
    fn 添加收藏() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let f = make_fav("记一下", "audio/a.mp3");
        add_favorite(&d, f.clone()).unwrap();
        let list = load_favorites(&d);
        assert_eq!(list, vec![f]);
    }

    #[test]
    fn 空备注拒绝写入() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let f = make_fav("   ", "audio/a.mp3");
        let r = add_favorite(&d, f);
        assert!(matches!(r, Err(StorageError::EmptyNote)));
        assert!(load_favorites(&d).is_empty(), "不应写入");
    }

    #[test]
    fn 删收藏_音频无引用则删() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let audio = make_audio(&d, "f1");
        let f = make_fav("n", &audio);
        add_favorite(&d, f.clone()).unwrap();
        assert!(d.join(&audio).exists());
        let ok = delete_favorite(&d, &f.id).unwrap();
        assert!(ok);
        assert!(!d.join(&audio).exists(), "音频应被删");
    }

    #[test]
    fn 删收藏_音频还被别处引用则保留() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let audio = make_audio(&d, "shared");
        let a = make_fav("n1", &audio);
        let b = make_fav("n2", &audio);
        add_favorite(&d, a.clone()).unwrap();
        add_favorite(&d, b.clone()).unwrap();
        delete_favorite(&d, &a.id).unwrap();
        assert!(d.join(&audio).exists(), "另一收藏仍引用，保留");
        assert_eq!(load_favorites(&d).len(), 1);
    }

    #[test]
    fn unlink_把来源引用置none() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let audio = make_audio(&d, "fa");
        let mut f = make_fav("n", &audio);
        f.source_message_id = Some("m_xxx".into());
        add_favorite(&d, f).unwrap();
        unlink_favorites_by_message(&d, "m_xxx").unwrap();
        let favs = load_favorites(&d);
        assert_eq!(favs.len(), 1);
        assert!(favs[0].source_message_id.is_none());
    }

    #[test]
    fn unlink_无匹配时不写文件() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let audio = make_audio(&d, "fb");
        let f = make_fav("n", &audio);
        add_favorite(&d, f).unwrap();
        let before = super::super::atomic::load_text(&d.join("favorites.json")).unwrap_or_default();
        unlink_favorites_by_message(&d, "完全无关的id").unwrap();
        let after = super::super::atomic::load_text(&d.join("favorites.json")).unwrap_or_default();
        assert_eq!(before, after, "无匹配应不写文件");
    }
}