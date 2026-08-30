// 配对状态：已配对 token（一对一，新配对顶替旧会话）。
//
// 配对唯一路径：免码 pair_request → PC 端弹窗确认（物理在场模型）→ 发 token。
// 配对码路径已于 1.8.x 移除（App 与 PC 面板同步去除；协议 §三 的
// pair / refresh_code 报文在服务端不再受理，收到即回错误并断开）。
// token 持久化在插件数据目录 token.json，宿主重启后 App 可凭 token 自动重连。

use std::path::PathBuf;

pub struct Pairing {
    /// 已配对 token（hex，64 字符）；None = 尚未配对
    token: Option<String>,
    /// token.json 路径（数据目录缺失时 None，token 只存内存）
    token_path: Option<PathBuf>,
}

fn data_dir() -> Option<PathBuf> {
    std::env::var("VA_PLUGIN_DATA_DIR_LAN_REMOTE")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn gen_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Pairing {
    /// 加载（有持久化 token 则带出）
    pub fn load() -> Self {
        let token = data_dir().and_then(|dir| {
            let path = dir.join("token.json");
            std::fs::read_to_string(&path).ok().and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| {
                        v.get("token")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    })
            })
        });
        Self {
            token: token.filter(|t| !t.is_empty()),
            token_path: data_dir().map(|dir| dir.join("token.json")),
        }
    }

    /// 校验重连 token
    pub fn check_token(&self, token: &str) -> bool {
        self.token.as_deref() == Some(token.trim())
    }

    /// PC 端弹窗确认通过后直接发 token（需物理在场点击允许）。
    /// 作废旧 token（一对一，新配对顶替旧会话）。
    pub fn approve(&mut self) -> String {
        let token = gen_token();
        self.token = Some(token.clone());
        self.save_token();
        token
    }

    fn save_token(&self) {
        let Some(path) = &self.token_path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::json!({ "token": self.token });
        if let Err(e) = std::fs::write(path, serde_json::to_string_pretty(&json).unwrap_or_default()) {
            eprintln!("[lan-remote] token 持久化失败: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pairing 直接构造太绕，测试走 load（数据目录 env 未设置 → token_path None）

    #[test]
    fn 免码配对发token且顶替旧会话() {
        let mut p = Pairing::load();
        let t1 = p.approve();
        assert_eq!(t1.len(), 64, "token 应为 32 字节 hex");
        assert!(p.check_token(&t1));
        assert!(!p.check_token("0000"));
        // 新配对顶替旧 token
        let t2 = p.approve();
        assert_ne!(t1, t2);
        assert!(!p.check_token(&t1), "旧 token 应被顶替作废");
        assert!(p.check_token(&t2));
    }
}
