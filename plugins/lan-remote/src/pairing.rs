// 配对状态：6 位配对码 + 已配对 token（一对一，新配对顶替旧会话）。
//
// token 持久化在插件数据目录 token.json，宿主重启后 App 可凭 token 自动重连。
// 配对码经宿主能力桥 set_own_config 写入 display 字段，在 PC 设置面板上屏。

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 配对码刷新最小间隔（防局域网内恶意刷新骚扰）
const CODE_REFRESH_COOLDOWN: Duration = Duration::from_secs(5);
/// 连续配对失败次数上限，超过后进入冷却
const MAX_FAILS: u32 = 5;
/// 配对失败冷却时长（爆破防护：6 位码空间 1e6，冷却下爆破不可行）
const FAIL_COOLDOWN: Duration = Duration::from_secs(30);

pub struct Pairing {
    /// 当前 6 位配对码（数字字符串）
    code: String,
    /// 已配对 token（hex，64 字符）；None = 尚未配对
    token: Option<String>,
    /// 上次配对码刷新时间
    last_refresh: Instant,
    /// 连续配对失败次数与冷却截止
    fail_count: u32,
    fail_cooldown_until: Option<Instant>,
    /// token.json 路径（数据目录缺失时 None，token 只存内存）
    token_path: Option<PathBuf>,
}

fn data_dir() -> Option<PathBuf> {
    std::env::var("VA_PLUGIN_DATA_DIR_LAN_REMOTE")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn gen_code() -> String {
    use rand::Rng;
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000u32))
}

fn gen_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Pairing {
    /// 加载（有持久化 token 则带出，配对码总是新生成）
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
        let mut p = Self {
            code: gen_code(),
            token: token.filter(|t| !t.is_empty()),
            last_refresh: Instant::now()
                .checked_sub(CODE_REFRESH_COOLDOWN)
                .unwrap_or_else(Instant::now),
            fail_count: 0,
            fail_cooldown_until: None,
            token_path: data_dir().map(|dir| dir.join("token.json")),
        };
        // 上一会话的旧码不作数（token 重连不受影响），启动即换新码
        p.code = gen_code();
        p
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// 刷新配对码（带冷却）。返回新码；冷却中返回 None。
    pub fn refresh_code(&mut self) -> Option<String> {
        let now = Instant::now();
        if now.duration_since(self.last_refresh) < CODE_REFRESH_COOLDOWN {
            return None;
        }
        self.code = gen_code();
        self.last_refresh = now;
        Some(self.code.clone())
    }

    /// 用配对码换 token：码正确 → 生成新 token（作废旧 token）并持久化。
    /// 码错误计数，超限进入冷却。Err 为给 App 的中文错误消息。
    pub fn pair(&mut self, code: &str) -> Result<String, String> {
        if let Some(until) = self.fail_cooldown_until {
            if Instant::now() < until {
                let secs = until.duration_since(Instant::now()).as_secs() + 1;
                return Err(format!("尝试过于频繁，请 {secs} 秒后再试"));
            }
            self.fail_cooldown_until = None;
            self.fail_count = 0;
        }
        if code.trim() == self.code {
            self.fail_count = 0;
            let token = gen_token();
            self.token = Some(token.clone());
            self.save_token();
            // 配对码即用即弃：成功后立即换新（旧码泄露也无用）
            self.code = gen_code();
            self.last_refresh = Instant::now();
            Ok(token)
        } else {
            self.fail_count += 1;
            if self.fail_count >= MAX_FAILS {
                self.fail_cooldown_until = Some(Instant::now() + FAIL_COOLDOWN);
                self.fail_count = 0;
            }
            Err("配对码错误，请查看 PC 端设置面板".to_string())
        }
    }

    /// 校验重连 token
    pub fn check_token(&self, token: &str) -> bool {
        self.token.as_deref() == Some(token.trim())
    }

    /// PC 端弹窗确认通过后直接发 token（免配对码路径；与 pair 同安全级别：
    /// 需物理在场点击允许）。作废旧 token，配对码即用即弃。
    pub fn approve(&mut self) -> String {
        self.fail_count = 0;
        let token = gen_token();
        self.token = Some(token.clone());
        self.save_token();
        self.code = gen_code();
        self.last_refresh = Instant::now();
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
    fn 配对码为6位数字() {
        let p = Pairing::load();
        assert_eq!(p.code().len(), 6);
        assert!(p.code().chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn 正确码换token且码作废() {
        let mut p = Pairing::load();
        let code = p.code().to_string();
        let t1 = p.pair(&code).expect("正确码应配对成功");
        assert_eq!(t1.len(), 64, "token 应为 32 字节 hex");
        // 同一个码第二次使用应失败（已作废）
        assert!(p.pair(&code).is_err());
        // 新 token 校验通过
        assert!(p.check_token(&t1));
        assert!(!p.check_token("0000"));
    }

    #[test]
    fn 新配对顶替旧token() {
        let mut p = Pairing::load();
        let code1 = p.code().to_string();
        let t1 = p.pair(&code1).unwrap();
        let code2 = p.code().to_string();
        let t2 = p.pair(&code2).unwrap();
        assert_ne!(t1, t2);
        assert!(!p.check_token(&t1), "旧 token 应被顶替作废");
        assert!(p.check_token(&t2));
    }

    #[test]
    fn 刷新冷却生效() {
        let mut p = Pairing::load();
        assert!(p.refresh_code().is_some(), "首次刷新应成功");
        assert!(p.refresh_code().is_none(), "冷却期内应拒绝");
    }
}
