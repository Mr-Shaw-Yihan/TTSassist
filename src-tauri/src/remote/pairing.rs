// 配对状态：已配对设备记录（一对一，新配对顶替旧会话）。
//
// 配对唯一路径：免码 pair_request → PC 端弹窗确认（物理在场模型）→ 发 token。
// 配对码路径已于 1.8.x 移除（协议 §三 的 pair / refresh_code 报文不再受理，
// 收到即回错误并断开）。记录持久化在本体数据目录 remote-pairing.json，
// 宿主重启后 App 可凭 token 自动重连。
//
// 设备记忆增强（原插件仅存单 token）：记录扩为 {token, device, paired_at, last_seen}，
// 遥控页据此展示「已配对设备 · 最近连接」；旧 lan-remote 的 token.json 由
// mod.rs::purge_legacy_plugin 迁移为无设备名的记录（老用户免重配）。

use std::path::{Path, PathBuf};

/// 配对文件名（本体数据目录下）
pub const FILE: &str = "remote-pairing.json";

/// 一条配对记录：token + 设备名 + 配对时间 + 最近连接时间（ISO8601）
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairRecord {
    /// 已配对 token（hex，64 字符）
    pub token: String,
    /// 配对时的设备名（App pair_request 上报；旧 token 迁移时为 None）
    #[serde(default)]
    pub device: Option<String>,
    /// 配对通过时间
    #[serde(default)]
    pub paired_at: Option<String>,
    /// 最近一次 hello 命中（重连）时间
    #[serde(default)]
    pub last_seen: Option<String>,
}

pub struct Pairing {
    /// 当前配对记录；None = 尚未配对
    record: Option<PairRecord>,
    /// remote-pairing.json 路径（数据目录缺失时 None，记录只存内存）
    path: Option<PathBuf>,
}

fn now_iso() -> String {
    chrono::Local::now().to_rfc3339()
}

fn gen_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Pairing {
    /// 从本体数据目录加载（有持久化记录则带出）
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE);
        let record = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| {
                // PowerShell 等工具写出的 UTF-8 可能带 BOM，serde_json 不认，先剥掉
                serde_json::from_str::<PairRecord>(s.trim_start_matches('\u{FEFF}')).ok()
            })
            .filter(|r: &PairRecord| !r.token.is_empty());
        Self {
            record,
            path: Some(path),
        }
    }

    /// 是否已配对（存在有效 token）
    pub fn is_paired(&self) -> bool {
        self.record.is_some()
    }

    /// 已配对设备名（旧 token 迁移无设备名时为 None）
    pub fn device(&self) -> Option<String> {
        self.record.as_ref().and_then(|r| r.device.clone())
    }

    /// 配对时间（ISO8601）
    pub fn paired_at(&self) -> Option<String> {
        self.record.as_ref().and_then(|r| r.paired_at.clone())
    }

    /// 最近连接时间（ISO8601）
    pub fn last_seen(&self) -> Option<String> {
        self.record.as_ref().and_then(|r| r.last_seen.clone())
    }

    /// 校验重连 token
    pub fn check_token(&self, token: &str) -> bool {
        self.record.as_ref().map(|r| r.token.as_str()) == Some(token.trim())
    }

    /// hello 命中：刷新最近连接时间并落盘
    pub fn touch(&mut self) {
        if let Some(r) = self.record.as_mut() {
            r.last_seen = Some(now_iso());
        }
        self.save();
    }

    /// PC 端弹窗确认通过后发 token（需物理在场点击允许）。
    /// 写入设备名与配对时间，作废旧记录（一对一，新配对顶替旧会话）。
    pub fn approve(&mut self, device: &str) -> String {
        let token = gen_token();
        let now = now_iso();
        self.record = Some(PairRecord {
            token: token.clone(),
            device: Some(device.to_string()),
            paired_at: Some(now.clone()),
            last_seen: Some(now),
        });
        self.save();
        token
    }

    fn save(&self) {
        let (Some(path), Some(record)) = (&self.path, &self.record) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = crate::storage::atomic::write_json_pretty(path, record) {
            log_error!("[remote] 配对记录持久化失败: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 免码配对发token且顶替旧会话() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Pairing::load(dir.path());
        assert!(!p.is_paired());
        let t1 = p.approve("小明的手机");
        assert_eq!(t1.len(), 64, "token 应为 32 字节 hex");
        assert!(p.check_token(&t1));
        assert!(!p.check_token("0000"));
        assert_eq!(p.device().as_deref(), Some("小明的手机"));
        assert!(p.paired_at().is_some());
        // 新配对顶替旧 token
        let t2 = p.approve("另一台设备");
        assert_ne!(t1, t2);
        assert!(!p.check_token(&t1), "旧 token 应被顶替作废");
        assert!(p.check_token(&t2));
        assert_eq!(p.device().as_deref(), Some("另一台设备"));
    }

    #[test]
    fn 记录持久化并可读回() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Pairing::load(dir.path());
        let t = p.approve("持久化设备");
        // 重新加载应带出同一记录
        let reloaded = Pairing::load(dir.path());
        assert!(reloaded.is_paired());
        assert!(reloaded.check_token(&t));
        assert_eq!(reloaded.device().as_deref(), Some("持久化设备"));
    }

    #[test]
    fn touch刷新最近连接时间() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Pairing::load(dir.path());
        p.approve("设备A");
        let before = p.last_seen();
        p.touch();
        let after = p.last_seen();
        assert!(before.is_some() && after.is_some());
        // last_seen 允许相等（同一秒内），但不应为 None
        assert!(after.unwrap() >= before.unwrap());
    }
}
