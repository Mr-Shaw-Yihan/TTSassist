// messages.json 读写
//
// 删除消息的连带逻辑（设计稿 1.7）：
// 1. 找到目标消息（含其 audio_path），找不到返回 false
// 2. 从 messages.json 过滤掉该条，原子写回
// 3. 调 favorites::unlink_favorites_by_message，把所有 source_message_id == 它 的收藏置 None
// 4. 判断该消息的 audio_path 是否还被引用（扫 messages 剩余 + favorites）
// 5. 无引用 → 删音频文件
// 顺序敏感：先写回 JSON 再删音频，最坏情况是"JSON 已删但音频残留"，无害；反之不可接受。

use std::path::Path;
use super::atomic::{write_json_pretty, Result};
use super::audio_gc::maybe_delete_audio;
use super::favorites;
use super::types::Message;

const FILE: &str = "messages.json";

fn path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(FILE)
}

/// 读全部消息，按存储顺序返回。文件不存在返回空 Vec。
pub fn load_messages(data_dir: &Path) -> Vec<Message> {
    let p = path(data_dir);
    if !p.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 分页读消息：取 before_id（不含该条）之前的最近 limit 条，按存储顺序（旧→新）返回。
/// before_id=None 表示取到最新；limit=None/0 表示不限（退化为读全部）。
/// before_id 不存在时（如已被删）退化为取到最新。返回 (窗口消息, 前面是否还有更早的)。
pub fn load_messages_page(
    data_dir: &Path,
    limit: Option<usize>,
    before_id: Option<&str>,
) -> (Vec<Message>, bool) {
    let all = load_messages(data_dir);
    let end = match before_id {
        Some(id) => all.iter().position(|m| m.id == id).unwrap_or(all.len()),
        None => all.len(),
    };
    let start = match limit {
        Some(n) if n > 0 => end.saturating_sub(n),
        _ => 0,
    };
    (all[start..end].to_vec(), start > 0)
}

/// 追加一条消息并原子写回。
pub fn add_message(data_dir: &Path, message: Message) -> Result<()> {
    let mut list = load_messages(data_dir);
    list.push(message);
    write_json_pretty(&path(data_dir), &list)
}

/// 删除一条消息：连带(收藏置 None + 音频无引用则删)。
/// 返回是否真的删了（false 表示没找到该 id）。
pub fn delete_message(data_dir: &Path, id: &str) -> Result<bool> {
    let list = load_messages(data_dir);
    let target = list.iter().find(|m| m.id == id).cloned();
    let Some(target) = target else {
        return Ok(false);
    };

    // 1. 过滤掉目标，原子写回
    let remaining: Vec<Message> = list.into_iter().filter(|m| m.id != id).collect();
    write_json_pretty(&path(data_dir), &remaining)?;

    // 2. 收藏里的来源引用置 None
    favorites::unlink_favorites_by_message(data_dir, id)?;

    // 3. 该消息音频无引用则删（先落数据再清文件）
    maybe_delete_audio(data_dir, &target.audio_path)?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{gen_id, now_iso};
    use tempfile::tempdir;

    fn make_msg(content: &str, audio_path: &str) -> Message {
        Message {
            id: gen_id("m"),
            content: content.into(),
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
        let list = load_messages(&dir.path().to_path_buf());
        assert!(list.is_empty());
    }

    #[test]
    fn 追加顺序正确() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let a = make_msg("第一条", "audio/a.mp3");
        let b = make_msg("第二条", "audio/b.mp3");
        add_message(&d, a.clone()).unwrap();
        add_message(&d, b.clone()).unwrap();
        let list = load_messages(&d);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], a);
        assert_eq!(list[1], b);
    }

    #[test]
    fn 删除已存在返回真() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let m = make_msg("x", "audio/x.mp3");
        add_message(&d, m.clone()).unwrap();
        let ok = delete_message(&d, &m.id).unwrap();
        assert!(ok);
        assert!(load_messages(&d).is_empty());
    }

    #[test]
    fn 删除不存在返回假() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let ok = delete_message(&d, "不存在的id").unwrap();
        assert!(!ok);
    }

    #[test]
    fn 删消息连带_收藏来源置none且数量不变() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let audio = make_audio(&d, "msg1");
        let m = make_msg("hi", &audio);
        add_message(&d, m.clone()).unwrap();

        // 建一条收藏，来源指向这条消息
        let f = super::super::types::Favorite {
            id: gen_id("f"),
            source_message_id: Some(m.id.clone()),
            note: "n".into(),
            audio_path: make_audio(&d, "fav1"),
            created_at: now_iso(),
            hotkey: None,
        };
        favorites::add_favorite(&d, f.clone()).unwrap();

        // 删消息：音频 msg1 被该消息独占引用，删消息后应被删
        delete_message(&d, &m.id).unwrap();

        let favs = favorites::load_favorites(&d);
        assert_eq!(favs.len(), 1, "收藏本身保留");
        assert!(favs[0].source_message_id.is_none(), "来源引用被置 None");
    }

    #[test]
    fn 删消息后音频无引用则删() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let audio = make_audio(&d, "m1");
        let m = make_msg("hi", &audio);
        add_message(&d, m.clone()).unwrap();
        assert!(d.join(&audio).exists());
        delete_message(&d, &m.id).unwrap();
        assert!(!d.join(&audio).exists(), "音频应被删");
    }

    #[test]
    fn 删消息但其音频被收藏引用则保留() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        let audio = make_audio(&d, "shared");
        let m = make_msg("hi", &audio);
        add_message(&d, m.clone()).unwrap();
        // 收藏也引用同一音频（极端场景：导入同一文件）
        let f = super::super::types::Favorite {
            id: gen_id("f"),
            source_message_id: None,
            note: "n".into(),
            audio_path: audio.clone(),
            created_at: now_iso(),
            hotkey: None,
        };
        favorites::add_favorite(&d, f).unwrap();

        delete_message(&d, &m.id).unwrap();
        assert!(d.join(&audio).exists(), "被收藏引用，音频保留");
    }

    #[test]
    fn 分页读消息() {
        let dir = tempdir().unwrap();
        let d = dir.path().to_path_buf();
        // 写入 5 条（存储顺序即时间序）
        let mut ids = Vec::new();
        for i in 0..5 {
            let m = make_msg(&format!("msg{i}"), &format!("audio/p{i}.mp3"));
            ids.push(m.id.clone());
            add_message(&d, m).unwrap();
        }

        // 取最新 2 条：应为后两条，且 has_more
        let (page, more) = load_messages_page(&d, Some(2), None);
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].content, "msg3");
        assert_eq!(page[1].content, "msg4");
        assert!(more);

        // 以 msg2 为上界向前翻：取到 msg0/msg1
        let (page, more) = load_messages_page(&d, Some(2), Some(&ids[2]));
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].content, "msg0");
        assert_eq!(page[1].content, "msg1");
        assert!(!more, "msg0 之前没有更早的了");

        // 窗口小于剩余量：上界 msg4，limit 2 → msg2/msg3，还有更早
        let (page, more) = load_messages_page(&d, Some(2), Some(&ids[4]));
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].content, "msg2");
        assert_eq!(page[1].content, "msg3");
        assert!(more);

        // 上界 id 不存在（已被删）→ 退化为取到最新
        let (page, _) = load_messages_page(&d, Some(2), Some("不存在的id"));
        assert_eq!(page[1].content, "msg4");

        // limit=None 读全部
        let (page, more) = load_messages_page(&d, None, None);
        assert_eq!(page.len(), 5);
        assert!(!more);
    }
}