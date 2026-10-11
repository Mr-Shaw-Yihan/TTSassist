# 存储布局与迁移 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: 用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐 Task 实现。步骤用 `- [ ]` 勾选跟踪。

**Goal:** 把「配置数据（%APPDATA%）」与「内容数据（跟随安装位置）」分两根落地，并把插件代码与插件重数据（模型/运行环境）分离，配首启数据检测与可回退迁移、残留数据可见可清理。

**Architecture:** 新增单一路径权威模块 `storage::paths`（`StoragePaths`：config_root / content_root / plugins_root + 各 getter + 解析优先级 + 不可写回退）；`AppState` 增 `config_dir` 并把现有 `data_dir` 语义收敛为「内容根」（19 个下游模块因此零改动即自动迁移），只有读配置的 3 处显式改用 `config_dir`。迁移在**首次启动**检测并执行（先复制校验、后把原件移入 `.migrated-backup-*`，绝不删除）。

**Tech Stack:** Rust（Tauri v2、`std::fs`、`tempfile` 单测）、TypeScript/React（设置页与卸载弹窗）、PowerShell（存储护栏）。

**设计依据：** `doc/存储布局与迁移设计.md`（六条契约、目标布局、迁移流程、验收判据）。

---

## 前置约定（全计划共享，命名必须一致）

| 概念 | 类型/函数 | 位置 | 含义 |
|---|---|---|---|
| 配置根 | `StoragePaths::config_root` | `src-tauri/src/storage/paths.rs` | `%APPDATA%\com.voiceassist.app`，只放 `settings.json`、`remote-pairing.json`、`storage.json` 指针、`.migrated-backup-*` |
| 内容根 | `StoragePaths::content_root` | 同上 | 默认 `<exe>\data`，可被 `VA_STORAGE_DIR` / 指针 / 不可写回退改变 |
| 插件根 | `StoragePaths::plugins_root` | 同上 | `<exe>\plugins`（沿用现有 `resolve_plugins_root()` 语义） |
| 下游参数 | `AppState::data_dir` | `src-tauri/src/commands/mod.rs` | **语义 = 内容根**（名字暂不改，Task 8 才做机械改名），`AppState::config_dir` = 配置根 |
| 指针文件 | `config_root/storage.json` | JSON `{"content_root": "D:\\...", "migrated_from": "...", "updated_at": "..."}` | 只有用户改过位置或迁移过才存在 |
| 归属清单 | 内容根 | — | `messages.json`、`favorites.json`、`subtitle_history.json`、`audio/`、`voice_samples/`、`logs/`、`cache/`、`downloads/`、`plugin-data/` |
| 归属清单 | 配置根 | — | `settings.json`（含 `plugin_config`）、`remote-pairing.json`、`storage.json` |
| 归属清单 | 插件根 | — | `registry.json` + `<id>/manifest.json` + `<id>/plugin.dll`（代码，与 dll 同生死） |
| 归属清单 | 内容根/plugin-data | — | `<id>/`：模型、运行环境、音色包（**卸载插件默认保留**） |

**每条 Task 完成的判据都是同一串命令**（在 `src-tauri\` 下，PowerShell）：

```powershell
$env:CARGO_HTTP_CHECK_REVOKE='false'
$env:CARGO_TARGET_DIR='d:\Docs\Project\TTKook\TTKook-mimo-plugin\src-tauri\target'
cargo test --lib 2>&1 | Select-String 'test result|^error|^warning'
```

基线（本计划起点，已实跑）：**182 passed / 0 failed / 3 ignored**，零 warning。前端基线：`tsc` 零输出、`vitest 9 files / 71 tests`、`npm run build` 成功。护栏基线：BOM ✓、日志 ✓、基座 57 条持平。

⚠ **写代码前先读这一条**（Task 1 实跑拦下来的）：本计划里所有新代码及其测试**一律用中立夹具名
`demo-tts`**，不得用 `genie-tts` / `MiniMax` 等真实引擎名。原因：基座护栏只允许残留变少，
给基座新增一处引擎命中就会红 —— 而路径/存储类逻辑根本不需要真实引擎名。

---

### Task 1: 路径权威模块 `storage::paths`

**Files:**
- Create: `src-tauri/src/storage/paths.rs`
- Modify: `src-tauri/src/storage/mod.rs`（加 `pub mod paths;`，并把 `ensure_data_dirs` 改为转发到新模块）

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/storage/paths.rs` 末尾写测试模块（先只写测试，实现留空）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn 内容根优先级_环境变量高于指针高于默认() {
        let (root, fb) = decide_content_root(
            Some(PathBuf::from("E:\\env")),
            Some(PathBuf::from("D:\\pointer")),
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\fallback"),
            &|_| true,
        );
        assert_eq!(root, PathBuf::from("E:\\env"));
        assert!(!fb, "显式指定不算回退");

        let (root, fb) = decide_content_root(
            None,
            Some(PathBuf::from("D:\\pointer")),
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\fallback"),
            &|_| true,
        );
        assert_eq!(root, PathBuf::from("D:\\pointer"));
        assert!(!fb);

        let (root, _) = decide_content_root(
            None, None,
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\fallback"),
            &|_| true,
        );
        assert_eq!(root, PathBuf::from("C:\\exe\\data"));
    }

    #[test]
    fn 不可写时回退并标记() {
        // 只有默认位置不可写才回退；指针位置不可写同样回退，但标记要带上真实原因
        let (root, fb) = decide_content_root(
            None,
            Some(PathBuf::from("E:\\gone")),
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\Users\\x\\AppData\\Local\\com.voiceassist.app\\data"),
            &|p: &Path| p != Path::new("E:\\gone"),
        );
        assert_eq!(root, PathBuf::from("C:\\Users\\x\\AppData\\Local\\com.voiceassist.app\\data"));
        assert!(fb, "非默认位置失效必须标记，界面才能说明原因");
    }

    #[test]
    fn 插件目录名安全化_与manifest_id规则同口径() {
        assert_eq!(plugin_dir_name("demo-tts"), "demo-tts");
        assert_eq!(plugin_dir_name("Demo TTS/中文"), "Demo_TTS___");
        // ↑ 实测值：中间那个下划线来自空格，尾部只 3 个（斜杠 + 两个汉字）。
        //   计划初稿写的 "MiniMaxTTS____" 与实际都不符，已按实跑校正为上面两行。
        // 路径穿越必须被吃掉，而不是照原样拼进去
        assert!(!plugin_dir_name("..\\evil").contains('\\'));
        assert!(!plugin_dir_name("../evil").contains('/'));
    }

    #[test]
    fn 各归属路径不共用根() {
        let p = StoragePaths::from_dirs(
            PathBuf::from("C:\\cfg"), PathBuf::from("D:\\app\\data"),
            PathBuf::from("D:\\app\\plugins"), false,
        );
        assert_eq!(p.settings_file(), PathBuf::from("C:\\cfg\\settings.json"));
        assert_eq!(p.messages_file(), PathBuf::from("D:\\app\\data\\messages.json"));
        assert_eq!(p.audio_dir(), PathBuf::from("D:\\app\\data\\audio"));
        assert_eq!(p.plugin_data_dir("genie-tts"), PathBuf::from("D:\\app\\data\\plugin-data\\genie-tts"));
        // 迁移中间产物留在配置根（跟 settings.json 同盘，回退时不用跨盘找）
        assert_eq!(p.backup_dir("20261011"), PathBuf::from("C:\\cfg\\.migrated-backup-20261011"));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib 2>&1 | Select-String '^error|cannot find'`
Expected: 编译失败，`cannot find type \`StoragePaths\` in this scope` / `cannot find function \`decide_content_root\``（约 5 条）。

- [ ] **Step 3: 写实现**

`src-tauri/src/storage/paths.rs` 全文（测试块保留在末尾）：

```rust
// 存储路径的唯一权威（设计文档 doc/存储布局与迁移设计.md §三契约 1）。
//
// 为什么要单独一个模块：全项目原本只有一处向系统要数据目录（lib.rs 的 app_data_dir），
// 但「数据放在哪」的知识散落在了 19 个模块的 join("audio") / join("voice_samples") 里。
// 配置与内容分根之后，任何一处自己拼路径都会静默地把数据写错地方——所以禁止，所以在这里集中。
//
// 规则（AGENTS.md「存储契约」同步记载）：
//   1. 业务代码不得调用 app_data_dir() / current_exe() 来拼数据路径；
//   2. 记录型文件里不得写绝对路径（audio_path 等一律相对内容根）；
//   3. 新增数据目录先加到本模块，再使用。

use std::path::{Path, PathBuf};

/// 三根的持有者。由 `StoragePaths::resolve` 在启动时构造一次，之后只读。
#[derive(Debug, Clone, PartialEq)]
pub struct StoragePaths {
    /// %APPDATA%\com.voiceassist.app —— 只放小而敏感的配置文件
    pub config_root: PathBuf,
    /// 内容与重数据的根，默认跟随安装位置（<exe>\data）
    pub content_root: PathBuf,
    /// 插件代码根（manifest + dll + registry），跟随安装位置
    pub plugins_root: PathBuf,
    /// 内容根不是"用户期望的默认/指针位置"而为不可写等原因回退时为 true → 界面必须说明原因
    pub content_fallback: bool,
}

impl StoragePaths {
    /// 测试与非注入场景用的直构器
    pub fn from_dirs(
        config_root: PathBuf,
        content_root: PathBuf,
        plugins_root: PathBuf,
        content_fallback: bool,
    ) -> Self {
        Self { config_root, content_root, plugins_root, content_fallback }
    }

    // ── 配置根下的条目 ─────────────────────────────────
    pub fn settings_file(&self) -> PathBuf { self.config_root.join("settings.json") }
    pub fn pairing_file(&self) -> PathBuf { self.config_root.join("remote-pairing.json") }
    /// 内容根指针（用户改过位置或迁移后才存在）
    pub fn pointer_file(&self) -> PathBuf { self.config_root.join("storage.json") }
    /// 迁移中间产物（原件移入、不删除，可回退）
    pub fn backup_dir(&self, stamp: &str) -> PathBuf {
        self.config_root.join(format!(".migrated-backup-{stamp}"))
    }

    // ── 内容根下的条目 ─────────────────────────────────
    pub fn messages_file(&self) -> PathBuf { self.content_root.join("messages.json") }
    pub fn favorites_file(&self) -> PathBuf { self.content_root.join("favorites.json") }
    pub fn subtitle_history_file(&self) -> PathBuf { self.content_root.join("subtitle_history.json") }
    pub fn audio_dir(&self) -> PathBuf { self.content_root.join("audio") }
    pub fn samples_dir(&self) -> PathBuf { self.content_root.join("voice_samples") }
    pub fn logs_dir(&self) -> PathBuf { self.content_root.join("logs") }
    pub fn cache_dir(&self) -> PathBuf { self.content_root.join("cache") }
    pub fn downloads_dir(&self) -> PathBuf { self.content_root.join("downloads") }
    pub fn plugin_data_root(&self) -> PathBuf { self.content_root.join("plugin-data") }
    pub fn plugin_data_dir(&self, id: &str) -> PathBuf { plugin_data_dir(&self.content_root, id) }

    /// 建齐内容根与各子目录（幂等，启动时调用一次）
    pub fn ensure_layout(&self) -> std::io::Result<()> {
        for dir in [
            self.config_root.as_path(), self.content_root.as_path(),
            self.audio_dir().as_path(), self.samples_dir().as_path(),
            self.logs_dir().as_path(), self.cache_dir().as_path(),
            self.plugin_data_root().as_path(), self.plugins_root.as_path(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// 决定内容根：环境变量 > 指针文件 > <exe>\data；被选位置不可写则回退 fallback。
/// `writable` 注入以便单测（真实实现是 `probe_writable`）。
pub fn decide_content_root(
    env_dir: Option<PathBuf>,
    pointer: Option<PathBuf>,
    exe_default: PathBuf,
    fallback: PathBuf,
    writable: &dyn Fn(&Path) -> bool,
) -> (PathBuf, bool) {
    // 显式指定的位置（env / 指针）不可写时不静默改掉，而是回退 + 标记，让界面能说明原因
    if let Some(p) = env_dir.filter(|p| !p.as_os_str().is_empty()) {
        return if writable(&p) { (p, false) } else { (fallback, true) };
    }
    if let Some(p) = pointer.filter(|p| !p.as_os_str().is_empty()) {
        return if writable(&p) { (p, false) } else { (fallback, true) };
    }
    if writable(&exe_default) {
        return (exe_default, false);
    }
    (fallback, true)
}

/// 读指针文件内容；不存在/格式坏都返回 None（返回 None 就走默认位置）。
pub fn read_pointer(path: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{FEFF}')).ok()?;
    v.get("content_root")?.as_str().map(|s| PathBuf::from(s.trim()))
        .filter(|s| !s.as_os_str().is_empty())
}

/// 写指针文件（无 BOM，原子写由调用方 storage::atomic 负责；这里只提供内容）
pub fn pointer_json(content_root: &Path, migrated_from: Option<&str>) -> String {
    let v = serde_json::json!({
        "content_root": content_root.display().to_string(),
        "migrated_from": migrated_from,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// 插件重数据目录的唯一算法（结构体方法也转发到这里）。
/// 为什么单独提供自由函数：loader / 命令层拿到的常常是 `&Path` 而不是 `StoragePaths`，
/// 没有这个入口它们就会回去 `join("data")` —— 那正是本模块要禁止的事。
pub fn plugin_data_dir(content_root: &Path, id: &str) -> PathBuf {
    content_root.join("plugin-data").join(plugin_dir_name(id))
}

/// 插件目录名安全化：与 manifest.id 的 `a-zA-Z0-9-_` 规则同口径，其余字符换 `_`。
/// 防的是把 `../evil` 这类拼进路径（设计契约 4/5 的边界）。
pub fn plugin_dir_name(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// 真实可写探测：建目录后写一个临时文件再删掉（只清自己造的文件）。
pub fn probe_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".va-write-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
```

`src-tauri/src/storage/mod.rs` 顶部加 `pub mod paths;`，并把现有 `ensure_data_dirs` 换成转发（保留旧签名，Task 2 再全面替换调用点）：

```rust
/// 建齐布局所需目录（转发到 `paths::StoragePaths::ensure_layout`）。
/// 保留旧签名以便 Task 2 之前调用点不炸。
pub fn ensure_data_dirs(data_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    std::fs::create_dir_all(data_dir.join("audio"))?;
    std::fs::create_dir_all(data_dir.join("voice_samples"))?;
    Ok(())
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib 2>&1 | Select-String 'test result'`
Expected: `test result: ok. 188 passed; 0 failed; 3 ignored`（+6 例：上面 4 条 +
`布局建齐后各目录都存在`（原列在 Task 2 Step 1，实际一并写在本 Task）+
`指针文件坏内容当没有`：

```rust
    #[test]
    fn 指针文件坏内容当没有() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("storage.json");
        assert!(read_pointer(&p).is_none(), "不存在 → None");
        std::fs::write(&p, "not json").unwrap();
        assert!(read_pointer(&p).is_none(), "格式坏 → None，不能崩");
        std::fs::write(&p, "{\"content_root\":\"  \"}").unwrap();
        assert!(read_pointer(&p).is_none(), "空值 → None，走默认位置");
        std::fs::write(&p, "{\"content_root\":\"D:\\\\VA\\\\data\"}").unwrap();
        assert_eq!(read_pointer(&p), Some(PathBuf::from("D:\\VA\\data")));
    }
```

同时：`cargo check --lib --tests` 必须零 warning。本 Task 里这些 pub 项还没人用，
会在文件顶部加一条带说明的临时豁免 `#![allow(dead_code)]`（注释里写明“Task 2 接线后必须移除”），
**不得**靠拉低门禁标准过关。

- [ ] **Step 5: 提交**

```powershell
git add src-tauri/src/storage/paths.rs src-tauri/src/storage/mod.rs
git commit -m "feat(存储): 新增路径权威模块 storage::paths（配置/内容分根的解析与归属）"
```

---

### Task 2: 启动链接入两根（**行为不变**，只改解析来源）

> ⚠ **排序约束（计划初稿的缺陷，已修正）**：本 Task **不得**把 `content_root` 默认值直接改成
> `<exe>\data`。否则在迁移能力（Task 4）落地前，用户启动会看到“消息/音频/收藏全空”
> （数据其实还在 AppData）——等于我们自造一场“数据丢失”事故。
> 因此本 Task 里 `exe_default` 位置先传 `config_root`（行为与今天一致），只把**解析链**换成 paths；
> 把默认翻到 `<exe>\data` 的动作单独放在 **Task 4 之后、作为 Task 4 的 Step 6**，并与迁移提示同批发布。

**Files:**
- Modify: `src-tauri/src/lib.rs:28-60`（`resolve_plugins_root` 并入 paths）、`src-tauri/src/lib.rs:108-160`（setup）
- Modify: `src-tauri/src/commands/mod.rs:30-40`（`AppState` 加 `config_dir`）
- Modify: `src-tauri/src/commands/vbcable.rs:70`（唯一一处绕过注入自取目录）
- Modify: `src-tauri/src/storage/settings.rs`（读写改走 `config_dir`）
- Modify: `src-tauri/src/remote/pairing.rs`（配对文件改走 `config_dir`）

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/storage/paths.rs` 的 tests 里加一条集成向的断言（用真临时目录）：

```rust
#[test]
fn 布局建齐后各目录都存在() {
    let dir = tempfile::tempdir().unwrap();
    let p = StoragePaths::from_dirs(
        dir.path().join("cfg"), dir.path().join("data"), dir.path().join("plugins"), false,
    );
    p.ensure_layout().unwrap();
    for d in [p.config_root.clone(), p.content_root.clone(), p.audio_dir(), p.samples_dir(),
              p.logs_dir(), p.cache_dir(), p.plugin_data_root(), p.plugins_root.clone()] {
        assert!(d.is_dir(), "缺少目录 {}", d.display());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib 2>&1 | Select-String 'test result|panicked'`
Expected: 本步不再单独产生红证 —— 该测试已前置到 Task 1 Step 1 一起写。保留本步骤只为了
在接线前先确认目录结构一致；真正的红证是 Task 1 Step 2 的 `cannot find type \`StoragePaths\``。

- [ ] **Step 3: 实现启动解析**

`src-tauri/src/lib.rs` 内替换 `resolve_plugins_root()` 为统一解析：

```rust
/// 启动时解析三根。顺序：环境变量 > 配置根指针 > 安装位置默认；安装位置不可写回退 LOCALAPPDATA。
fn resolve_storage_paths(app: &tauri::AppHandle) -> storage::paths::StoragePaths {
    use storage::paths::*;
    let config_root = app.path().app_data_dir().expect("无法获取配置目录");
    let exe_dir = std::env::current_exe().expect("无法获取 exe 路径")
        .parent().expect("exe 无父目录").to_path_buf();

    let env_dir = std::env::var("VA_STORAGE_DIR").ok().map(PathBuf::from);
    let plugins_env = std::env::var("VA_PLUGINS_DIR").ok().map(PathBuf::from);
    let pointer = read_pointer(&config_root.join("storage.json"));

    let local = std::env::var("LOCALAPPDATA").ok()
        .map(|a| PathBuf::from(a).join("com.voiceassist.app").join("data"))
        .unwrap_or_else(|| exe_dir.join("data-fallback"));

    let (content_root, fallback) = decide_content_root(
        env_dir, pointer, exe_dir.join("data"), local, &probe_writable,
    );
    let plugins_root = plugins_env.unwrap_or_else(|| exe_dir.join("plugins"));
    let paths = StoragePaths::from_dirs(config_root, content_root, plugins_root, fallback);
    if let Err(e) = paths.ensure_layout() {
        log_warn!("创建存储目录失败: {e}");
    }
    if fallback {
        log_warn!(
            "内容数据回退到 {}（安装目录不可写或指定位置失效），设置页需向用户说明",
            paths.content_root.display()
        );
    }
    paths
}
```

`setup` 里把 `let data_dir = app.path().app_data_dir()?;` 换成：

```rust
            let paths = resolve_storage_paths(app);
            let (config_dir, data_dir) = (paths.config_root.clone(), paths.content_root.clone());
            storage::ensure_data_dirs(&data_dir)?;
            app.manage(paths.clone());
```

其中 `resolve_storage_paths` 本 Task 先传 `exe_default = config_root`（**行为不变**），
到 Task 4 Step 6 才改回 `exe_dir.join("data")`；代码里要带上这句提醒注释：

```rust
    // ⚠ 迁移能力（Task 4）就位前不能翻默认值，否则旧数据会“看不见”= 看起来像丢数据
    let exe_default = config_root.clone(); // Task 4 Step 6 改为 exe_dir.join("data")
```

并把 `AppState::new(data_dir.clone(), settings.clone())` 改为 `AppState::new(config_dir.clone(), data_dir.clone(), settings.clone())`；`storage::settings::load_settings(&data_dir)` 与 `plugins::migrate_legacy_engine(&data_dir)` 改传 `&config_dir`；`logging::init(&data_dir.join("logs"), …)` 改 `paths.logs_dir()`。

`commands/mod.rs`：

```rust
pub struct AppState {
    /// 配置根（settings / pairing / 指针）。迁移类命令读写配置时用。
    pub config_dir: PathBuf,
    /// 内容根（消息、音频、样本、日志、插件重数据）。默认跟随安装位置。
    pub data_dir: PathBuf,
    pub settings: RwLock<Settings>,
}
```

`commands/vbcable.rs:70` 去掉自取目录：

```rust
    let data_dir = state.data_dir.clone();   // 原：app.path().app_data_dir()
```

- [ ] **Step 4: 跑测试确认通过 + 全量门禁**

```powershell
cargo test --lib 2>&1 | Select-String 'test result|^warning'    # 期望 188 passed / 0 failed / 3 ignored（本 Task 不新增测试）
cargo check --lib --tests 2>&1 | Select-String 'warning|error'   # 期望无输出
cd ..; pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-base-hygiene.ps1   # 期望 ✓（paths.rs 无引擎名）
```

- [ ] **Step 5: 手工回归（本 Task 唯一的真机步骤）**

`npm run tauri dev` 后确认：**行为与今天完全一致**（本 Task 不翻默认）——
实测四项：① 设置能读写、发消息能出音频；② `%APPDATA%\com.voiceassist.app\` 下**不出现**
`storage.json`（只有改过位置或迁移过才写）；③ `messages.json` 仍在原位置；
④ **`<exe 同级>\data\` 不应被创建**（内容根仍 == 配置根，`ensure_layout` 不会多造目录）。
日志验证口径：`app.log` 只在设置里“诊断日志（支持模式）”开着时才写（默认关），
所以看 dev 的 stdout 或先打开该开关，**不得**把“没日志”当成回归信号。
再把 `paths.rs` 顶部的 `#![allow(dead_code)]` 注释里标明的移除时机说清楚（Task 3/4 接完 getter 后），
本 Task 结束时仍保留是正确的：多数 getter 确实还没调用方。

- [ ] **Step 6: 提交**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/commands/mod.rs src-tauri/src/commands/vbcable.rs src-tauri/src/storage/settings.rs src-tauri/src/remote/pairing.rs src-tauri/src/storage/paths.rs
git commit -m "feat(存储): 启动时解析配置/内容两根并接入 AppState（内容数据默认跟随安装位置）"
```

---

### Task 3: 插件重数据分离（`data/plugin-data/<id>`）+ 孤儿清理作用域锁死

**Files:**
- Modify: `src-tauri/src/plugins/loader.rs:67-78`、`:849-858`、`:951-960`（三处数据目录注入）
- Modify: `src-tauri/src/commands/plugins.rs:475`（离线资源包导入目标）、`:558`（清除失败资源）、卸载命令（新增 `delete_data` 参数）
- Modify: `src-tauri/src/plugins/manager.rs:822-845`（`sweep_orphan_dirs` 明确只处理 `plugins_root` 顶层目录）
- Test: 同文件 `mod tests`

- [ ] **Step 1: 写失败测试**

`loader.rs` 的测试块新增（若该文件无 tests 模块则新建 `#[cfg(test)] mod tests`）：

```rust
#[test]
fn 插件数据目录指向内容根的plugin_data而非插件目录内() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::storage::paths::StoragePaths::from_dirs(
        dir.path().join("cfg"), dir.path().join("data"), dir.path().join("plugins"), false,
    );
    assert_eq!(
        paths.plugin_data_dir("demo-tts"),
        dir.path().join("data").join("plugin-data").join("demo-tts")
    );
    // 关键：不再等于插件目录内的 data（旧行为会让“清理插件目录”顺手删掉几 GB 模型）
    assert_ne!(
        paths.plugin_data_dir("demo-tts"),
        paths.plugins_root.join("demo-tts").join("data")
    );
}
```

`manager.rs` 测试块新增（锁死作用域）：

```rust
#[test]
fn 孤儿清理不碰plugin_data目录() {
    let dir = tempfile::tempdir().unwrap();
    let plugins_root = dir.path().join("plugins");
    let stray_plugin = plugins_root.join("gone-plugin");
    let stray_data = dir.path().join("data").join("plugin-data").join("gone-plugin");
    std::fs::create_dir_all(&stray_plugin).unwrap();
    std::fs::create_dir_all(&stray_data).unwrap();
    std::fs::write(stray_data.join("model.bin"), b"x").unwrap();

    sweep_orphan_dirs(&plugins_root, &registry::Registry::default());

    assert!(!stray_plugin.exists(), "孤儿插件目录应被清理");
    assert!(stray_data.exists(), "插件重数据必须保留（用户可能就是特意选择保留的）");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib 2>&1 | Select-String 'panicked|assertion'`
Expected: 至少一条 `assertion failed: 插件重数据必须保留…`（当前 `sweep_orphan_dirs` 只扫 `plugins_root` 顶层，本条应通过；若第一步因 `sweep` 行为意外通过，就先把 Step 1 第二条当作**回归防护**保留，并在 commit message 里注明它是防护性测试而非红测）。

- [ ] **Step 3: 实现**

`loader.rs` 三处统一改为从 `StoragePaths` 取（`LoadedPlugin::load` 增加 `data_dir: &Path` 入参，由 `PluginManager` 传入；`manager.rs` 持有 `StoragePaths` 或至少持有 `content_root`）：

```rust
        // 1.5 插件重数据目录：<内容根>\plugin-data\<id>（与 dll 分离，卸载代码不牵连模型）
        let data_dir = plugin_data_dir_for(&data_root, &manifest.id);
```

其中 `plugin_data_dir_for` 直接复用 `paths::StoragePaths::plugin_data_dir`（若不便持有结构体，就暴露一个自由函数 `pub fn plugin_data_dir(content_root: &Path, id: &str) -> PathBuf`，两处共用一处实现）。

`plugins.rs:475` 离线资源包导入目标同样改为该函数；`:558` 清除失败资源改为在 `plugin_data` 下操作。卸载命令：

```rust
/// 卸载插件。`delete_data=false` 时**保留** plugin-data（几 GB 模型不连带删除）。
#[tauri::command]
pub async fn uninstall_plugin(
    id: String,
    delete_data: Option<bool>,
    app: AppHandle,
    plugins: State<'_, PluginManager>,
    paths: State<'_, StoragePaths>,
) -> Result<String, String> { /* 原逻辑 + 末尾按 delete_data 决定是否删 paths.plugin_data_dir(&id) */ }
```

`manager.rs::sweep_orphan_dirs` 顶部加断言式注释与防御（只接受 `plugins_root`，函数签名不变即可保证作用域）：

```rust
/// 清理孤儿插件目录：**只**遍历 `plugins_root` 顶层，永不触碰 `<内容根>\plugin-data\`。
/// 后者是用户可能刻意保留的模型/运行环境，只能由「存储」页显式清理（设计契约 4）。
```

- [ ] **Step 4: 跑测试确认通过**

Expected: `test result: ok. 190 passed; 0 failed; 3 ignored`（+2 例）。

- [ ] **Step 5: 提交**

```powershell
git add src-tauri/src/plugins/loader.rs src-tauri/src/plugins/manager.rs src-tauri/src/commands/plugins.rs src-tauri/src/storage/paths.rs
git commit -m "feat(插件): 插件重数据独立到 data/plugin-data 并让卸载可选保留（孤儿清理不再牵连模型）"
```

---

### Task 3b: 调用点归属重分配（翻默认前必做完）—— ✅ 已完成

实跑结果（一次正则 + 四处人工 + 机器审计）：14 处 `settings::update_setting/save_settings/load_settings`
调用改走 `config_dir`；`clone_voice.rs` 两个函数里**同时用到两个根**（样本文件=内容、
设置回写=配置），已拆开并注明；`remote-config-cache.json` 与读它的 `effective_help_url` 一并归**配置根**
（小而常读的运营配置，与 settings 同族）——修正了设计初稿把 `cache/` 一律归内容根的含糊说法。
审计输出（每条 settings 调用的目录参数）已全部是 config_dir；测试仍 192 passed / 零 warning；
`git diff --stat` 仅 28+/22-（证明脚本没把换行/BOM 改掉，那些文件本来无 BOM）。

⚠ 过程里两次被自己的工具打脸，记下来免得再犯：
1. .NET 的 `ReadAllText/WriteAllText` 不看 PowerShell 的 `cd`（Process CWD 仍是仓库根），
   第一次跑直接报“找不到路径”——必须用绝对路径（本项自已有记录，又踩一次）。
2. SearchReplace 里为了多带一行上下文，把 `let src = Path::new(...)` 与一段注释首行一起替掉了；
   靠 diff 逐行阅读才发现并补回。**改完必须读 diff**，不能只看编译通过。

剩下的护栏钩子（写进 Task 7）：存储护栏要新增一条规则 ——
`settings::(update_setting|save_settings|load_settings)` 的目录参数必须是 `config_dir`（或测试里的临时目录），
命中 `data_dir` 即红。这是本次唯一能防“静默分裂”的机械化手段（两个根都是 PathBuf，编译器帮不了）。

### Task 3c: 内容类字面量收口（Task 7 护栏上线前做完）—— ✅ 已完成

实跑结果：**业务代码里已零字面量**。新增 `paths` 里的单一来源：六个子目录名常量
（`AUDIO_DIR_NAME` 等）+ `audio_dir_in` / `samples_dir_in` / `downloads_dir_in`（只持有 `&Path` 时用）
+ `audio_rel(id, ext)` / `samples_rel(name, ext)`（记录里相对路径的格式）。
已改接：`clone_voice.rs`（样本目录 + 相对路径）、`favorite.rs`、`moss.rs`、
`tts_stream.rs`（建目录 + `stream_paths_for` 改用常量）、`vbcable.rs`、`storage::ensure_data_dirs`
（顺带把 `voice_samples` 也建上，幂等无害）。新用 1 例单测钉住“相对路径格式 = 数据格式”
（改它等于改 `messages.json` 里的值，必须配迁移）。测试：193 passed / 零 warning。

审计后残留的 `join("audio")` 全部在 **`#[cfg(test)]` 测脚手架**里（构造临时目录用）：
`audio_gc.rs:61-63`、`favorites.rs:111-112`、`messages.rs:100-101/214`、`manager.rs:927/943`、
`loader.rs:693`、`storage/mod.rs:39`。这些是合法用法，但 PowerShell 护栏区分不了
`#[cfg(test)]` 内外 —— **Task 7 的处置：把这七条当固定条目登进基线**（数量恒定、只许变少），
不靠改测试风格去迁就工具。

业务代码里仍有零散的 `join("audio")` / `join("voice_samples")` 等字面量（tts_stream.rs、moss.rs、
audio_gc.rs、clone_voice.rs、messages/favorites 内部），它们现在走 `state.data_dir`（=内容根）是对的，
但存储契约要求路径只来自 `storage::paths`。处理：给 `AppState` 加 `audio_dir()/samples_dir()/downloads_dir()`
转发到 `StoragePaths`（或由命令处 `app.state::<StoragePaths>()` 取），把这批字面量收敛掉，
否则 Task 7 的护栏一上就会一片红。

Task 3 先完成了插件重数据分离；本步把“哪些读写走配置根、哪些走内容根”逐个改对。
**因为现在两根仍相等，这一步全部是行为中性改动**；但 Task 4 Step 6 翻默以前必须做完，
否则会出现“settings 写到内容根、代码去配置根读”这类静默分裂。

走**配置根**（`state.config_dir` / `paths.settings_file()` / `paths.pairing_file()`）：

| 文件 | 行 | 内容 |
|---|---|---|
| `commands/plugins.rs` | 111 / 152 / 184 / 357 / 389 | `update_setting` / `save_settings`（卸载、配置保存、环境安装回写） |
| `commands/clone_voice.rs` | 74 / 76 / 114 / 116 | 导入与删除样本时回写 settings |
| `commands/floating_ball.rs` | 81 / 121 / 283 | 悬浮球偏好写入 |
| `commands/settings.rs` | 34 | 通用 `update_setting` 入口 |
| `commands/subtitle.rs` | 383 | 字幕设置回写 |
| `plugins/bridge.rs` | 359 | 能力桥改设置 |
| `hotkey.rs` | 147 / 226 / 302 / 384 | 快捷键与设备设置回写 |
| `remote/mod.rs` | 114 | 配对记录（Task 3 已改） |
| `lib.rs` | 已改 | `migrate_legacy_engine` / `load_settings` |

走**内容根**（`paths.*_dir()` / `paths.*_file()`）：`messages.json`、`favorites.json`、
`subtitle_history.json`、`audio/`、`voice_samples/`、`logs/`、`cache/`、`downloads/`（vbcable 已改）、
`plugin-data/<id>`（Task 3 已改）。

验收：改完跑一次全量门禁（期望测试数不变，纯改参数来源），再请用户确认一遍“设置仍可读”；
完成后才能进 Task 4 Step 6 翻默认。

### Task 4: 首启数据检测与可回退迁移 —— Step 1-5 已完成（bbb04c1），Step 6 待真机验收

**Files:**
- Create: `src-tauri/src/storage/migrate.rs`
- Modify: `src-tauri/src/storage/mod.rs`（`pub mod migrate;`）、`src-tauri/src/lib.rs`（setup 内接入）

- [ ] **Step 1: 写失败测试**

`src-tauri/src/storage/migrate.rs`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 状态判定四种组合() {
        assert_eq!(detect_state(false, false), StorageState::Fresh);
        assert_eq!(detect_state(true, false), StorageState::Current);
        assert_eq!(detect_state(false, true), StorageState::Migratable);
        assert_eq!(detect_state(true, true), StorageState::Conflict);
    }

    #[test]
    fn 旧项扫描只认清单里的条目() {
        let dir = tempfile::tempdir().unwrap();
        // settings.json 留在配置根，**不是**待迁移项（把它算进去会误判需要搬家）
        std::fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        assert!(!legacy_items(dir.path()).iter().any(|p| p.display().to_string().contains("settings.json")));
        assert!(legacy_items(dir.path()).is_empty());

        std::fs::create_dir_all(dir.path().join("audio")).unwrap();
        std::fs::write(dir.path().join("audio").join("m_1.wav"), b"x").unwrap();
        assert_eq!(legacy_items(dir.path()).len(), 1);
    }

    #[test]
    fn 迁移执行后原件进备份且记录可读() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let content = dir.path().join("data");
        std::fs::create_dir_all(legacy.join("audio")).unwrap();
        std::fs::write(legacy.join("messages.json"), b"[]").unwrap();
        std::fs::write(legacy.join("audio").join("m_1.wav"), b"abc").unwrap();

        let report = migrate_items(&legacy, &content, "20261011").unwrap();

        assert_eq!(report.moved, 2);
        assert!(content.join("messages.json").exists());
        assert!(content.join("audio").join("m_1.wav").exists());
        // 原件不删：移入配置根下的 .migrated-backup-<stamp>
        assert!(legacy.join(".migrated-backup-20261011").exists());
        assert!(!legacy.join("messages.json").exists());
    }

    #[test]
    fn 校验不过时不动原件() {
        // 用一个不可复制的项制造失败：目标已存在同名且是**不同内容**的文件
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let content = dir.path().join("data");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("messages.json"), b"[]").unwrap();
        std::fs::create_dir_all(&content).unwrap();
        let report = migrate_items_with_verify(&legacy, &content, "20261011", |_| false);
        assert_eq!(report.moved, 0, "校验失败一项都不该搬");
        assert!(legacy.join("messages.json").exists(), "原件必须原地不动");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Expected: 编译失败 —— `cannot find function \`detect_state\` in this scope` 等 4 条。

- [ ] **Step 3: 实现**

```rust
// 首启数据检测与迁移（设计文档 §四）。
// 为什么不在安装器里做：NSIS 里搬几 GB + 校验 + 回滚是事故温床；放首启则天然幂等、
// 有 UI 可交互、失败可重试。1.9 及以后每版启动都跑这道判定，即用户要求的"先做数据检测"。

use std::path::{Path, PathBuf};

/// 迁移判定结果。Conflict 必须让用户选，禁止静默二选一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageState { Current, Fresh, Migratable, Conflict }

pub fn detect_state(content_has_data: bool, legacy_present: bool) -> StorageState {
    match (content_has_data, legacy_present) {
        (false, false) => StorageState::Fresh,
        (true, false) => StorageState::Current,
        (false, true) => StorageState::Migratable,
        (true, true) => StorageState::Conflict,
    }
}

/// 待迁移清单 = 内容类条目（**不含 settings.json / remote-pairing.json / storage.json**）
pub fn legacy_items(legacy_root: &Path) -> Vec<PathBuf> {
    const FILES: [&str; 3] = ["messages.json", "favorites.json", "subtitle_history.json"];
    const DIRS: [&str; 5] = ["audio", "voice_samples", "logs", "cache", "downloads"];
    let mut out = Vec::new();
    for f in FILES {
        let p = legacy_root.join(f);
        if p.is_file() { out.push(p); }
    }
    for d in DIRS {
        let p = legacy_root.join(d);
        if dir_has_entries(&p) { out.push(p); }
    }
    out
}

fn dir_has_entries(p: &Path) -> bool {
    std::fs::read_dir(p).map(|mut r| r.next().is_some()).unwrap_or(false)
}

#[derive(Debug, Default, Clone)]
pub struct MigrationReport { pub moved: usize, pub bytes: u64, pub backup: PathBuf }

/// 校验通过才移原件；任何一项失败整批回退（删新副本、原件不动），下次仍可重跑
pub fn migrate_items(legacy_root: &Path, content_root: &Path, stamp: &str) -> Result<MigrationReport, String> {
    migrate_items_with_verify(legacy_root, content_root, stamp, |_| true)
}

pub fn migrate_items_with_verify(
    legacy_root: &Path,
    content_root: &Path,
    stamp: &str,
    verify: impl Fn(&Path) -> bool,
) -> Result<MigrationReport, String> {
    let items = legacy_items(legacy_root);
    let backup = legacy_root.join(format!(".migrated-backup-{stamp}"));
    let mut report = MigrationReport { moved: 0, bytes: 0, backup: backup.clone() };
    if items.is_empty() { return Ok(report); }

    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for src in &items {
        let rel = src.strip_prefix(legacy_root).map_err(|e| format!("路径归属异常: {e}"))?;
        let dst = content_root.join(rel);
        if let Some(parent) = dst.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        copy_into(src, &dst)?;
        if !verify(&dst) {
            rollback(&done)?;                        // 校验失败：删掉本次复制的，原件不动
            return Err(format!("迁移校验失败，已回退：{}", dst.display()));
        }
        report.bytes += dir_size(src);
        done.push((src.clone(), dst));
    }
    std::fs::create_dir_all(&backup).map_err(|e| format!("创建备份目录失败: {e}"))?;
    for (src, _) in done {
        let target = backup.join(src.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        std::fs::rename(&src, &target).map_err(|e| format!("移原件到备份失败: {e}"))?;
        report.moved += 1;
    }
    Ok(report)
}

fn copy_into(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() { crate::copy_dir_all(src, dst).map_err(|e| e.to_string()) }
    else { std::fs::copy(src, dst).map(|_| ()).map_err(|e| e.to_string()) }
}

fn rollback(done: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    for (_, dst) in done.iter().rev() {
        if dst.is_dir() { let _ = std::fs::remove_dir_all(dst); } else { let _ = std::fs::remove_file(dst); }
    }
    Ok(())
}

/// 递归求目录字节数（UI 只在按需查询时调用，不在启动路径上跑）。
/// 不跟符号链接：虚拟环境里常见链接，跟进去会重复计数甚至死循环。
pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(p) else { return 0 };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            total += dir_size(&entry.path());
        } else if ft.is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 迁移状态持有器：Task 5 的 storage_report 据此告知界面“该搬家了”
#[derive(Debug, Clone, Default)]
pub struct MigrationPending { pub state: StorageState }

impl MigrationPending {
    pub fn new(state: StorageState) -> Self { Self { state } }
}

impl std::default::Default for StorageState {
    /// 默认 Current：宁可显示“无需迁移”，也不要在没把握时弹迁移提示
    fn default() -> Self { StorageState::Current }
}
```

并在 `lib.rs` 的 `setup` 内、解析两根之后接入：

```rust
            // 首启数据检测（幂等）：需要搬家时先只记日志并置标记，UI 由 storage_report 取状态
            {
                let state = storage::migrate::detect_state(
                    storage::migrate::dir_has_entries(&paths.content_root),
                    !storage::migrate::legacy_items(&paths.config_root).is_empty(),
                );
                match state {
                    storage::migrate::StorageState::Migratable => {
                        app.manage(MigrationPending::new(state));
                        log_info!("检测到旧位置数据待迁移（内容根尚未使用）：等待用户确认");
                    }
                    storage::migrate::StorageState::Conflict => {
                        app.manage(MigrationPending::new(state));
                        log_warn!("新旧两处都有数据，需用户在设置页选择处置");
                    }
                    other => log_info!("存储状态：{:?}", other),
                }
            }
```

（两个补充事实：`copy_dir_all` 现在 `lib.rs:63` 是私有的，本 Task 要顺手改成
`pub(crate) fn copy_dir_all(...)`，否则 `crate::copy_dir_all` 编译不过；`StorageState` 已
`derive(Default)` 与上面代码一致，均不可省。）

- [ ] **Step 4: 跑测试确认通过**

Expected: `test result: ok. 194 passed; 0 failed; 3 ignored`（+4 例）。

- [ ] **Step 5: 提交**

```powershell
git add src-tauri/src/storage/migrate.rs src-tauri/src/storage/mod.rs src-tauri/src/lib.rs
git commit -m "feat(存储): 首启数据检测与可回退迁移执行器（先复制校验、后移入备份目录）"
```

- [ ] **Step 6: 翻默认——把内容根指向安装位置（与上面的迁移能力同批发布）**

> ⚠ 实施时发现排序缺陷：本步的真机验收要「看到 Migratable 提示与体积数字 → **执行迁移**」，
> 而“执行迁移”的命令要到 Task 5 才存在。正确执行顺序：Task 5 → Task 6 → 本步（翻默认）+ 真机验收，
> 不要把 Step 6 当成 Task 4 的尾巴单独跑。

这是全计划**唯一会让数据换地方**的一步，所以必须排在迁移能力之后：

```rust
    // 内容根默认跟随安装位置；不可写时回退 LOCALAPPDATA 并在界面说明
    let exe_default = exe_dir.join("data");   // Task 2 曾临时传 config_root（行为不变），现在才真算翻默认
```

跑一次真机全流程（设计文档 §九 的第 2、5 两条）：拿一份有旧数据的环境启动 →
看到 Migratable 提示与体积数字 → 执行迁移 → 消息/音频/收藏/克隆样本齐全、
**删一条消息仍能按引用计数删掉音频文件**；再跑一：迁移中途强杀进程 → 重启仍判 Migratable 可重跑。
两条都过才提交（不过则回到本 Task 修，不得先翻默认再补迁移）：

```powershell
git add src-tauri/src/lib.rs
git commit -m "feat(存储): 内容根默认改跟随安装位置（迁移能力已就位）"
```

**Step 6 实跑结果（2026-10-11 晚~10-12，提交 `ee97a7f` + `dad6bd1`）**

行为改为用户定的“首启自动搬完、只弹提示”（覆盖设计 §四 原先的“等用户确认”），
因为翻默认后老用户升级会先看到一个空的聊天列表。

| 验收项 | 结果 | 证据 |
|---|---|---|
| 首启自动搬迁（真实数据） | ✓ | 配置根的 audio/样本/日志 + 3 个 json 全部进了`.migrated-backup-20261011`，内容根 126 个音频到位 |
| 音频可回放 | ✓（修后） | 用户实听：新消息/旧消息/收藏均有声 |
| 删消息按引用回收音频 | ✓ | 双向对账：引用但缺文件 0、无人引用却在磁盘 0（125 消息 + 4 收藏 → 去重 126 = 磁盘 126） |
| Conflict 不静默二选一 | ✓ | 旧位置被放回一份**更早**的快照（与新位置 `messages.json` 34451 vs 34166），自动搬迁按规则**拒绝执行**，用户新删的 2 条消息没被复活 |
| toast 亲眼确认 | 待做 | 见下方“沙箱限制” |
| 第 7 步 急救程序退回 | 待做 | release 版正在编；同样需用户执行 |

真机才跳出来的四个缺陷（已全修）——共同点是 `cargo check`/`tsc`/单测当时全绿：
1. **asset: 作用域**：`tauri.conf.json` 只放行 `$APPDATA/**`，音频搬到 `<exe>\data` 后被拒→空播。
   改为运行时 `asset_protocol_scope().allow_directory(内容根, true)`（位置是动态推导的，静态配置表达不了）。
2. **播放错误被吞**：`a.play().catch(() => {})` 使按钮永远停在“播放中”。现在 `error` 与 reject 都收口状态并提示。
3. **prime_cache 传错根**：写内容根、读配置根，翻完就“写一个文件读另一个”，远程 help_url 覆盖静默失效。
   同时把 remote.rs 里名为 `data_dir` 实须配置根的参数改名（两个 `PathBuf` 编译器分不清）。
4. **`tauri dev` 起不来**：Task 5b 加了 `src/bin/` 后包内两个二进制，`cargo run` 无法自动选。
   补 `default-run = "voiceassist"`。**教训：`cargo check --bins` 绿不等于能跑，新增 bin 后必须实测 dev 启动。**

另外三个靠推演而不是靠跑发现的设计缺陷（`dad6bd1`，都有先红后绿的单测）：
- 断电重跑被误判成 Conflict（拷贝是先填新位置的，“新位置已有数据”不能当冲突依据）；
- 归档用 `rename` 入备份 → Windows 带 REPLACE 语义，**静默销毁上一轮归档**（回退凭据没了）→ 改 `free_backup_target` 改名存；
- 判身份只看字节数 → 我自己写的“内容不同”用例恰好同长（10 字节），冲突根本判不出→ 改为真比对内容。

沙箱限制（影响验收方式，不影响产品）：代理 shell 写不了 `%APPDATA%`（读/移出可以），
所以“再演示一次自动搬迁”与“急救程序退回”两步必须由用户在本地终端执行；seed 命令已给出。

两个挂账（本版不做，已告知用户）：
- 静态 scope `$APPDATA/**` 递归包含了 `settings.json`（各引擎 API Key），任何窗口 JS 理论上可用
  `asset:` 读到——翻默认前就有的口子，收口需改 `resolve_audio_url` 只白名单子目录。
- 本机 `AppData\plugin-data`（23.6 MB，含套一层的 `plugin-data\plugin-data`）是早期 dev 实例把
  内容根当 AppData 时留下的；新界面只读 `<内容根>\plugin-data`，所以它既不显示也不被清理。
  真实用户不会有（他们的插件数据一直在 `<安装目录>\plugins\<id>\data`，Task 3 会搬）。

---

### Task 5: 存储报告与残留治理命令 —— ✅ 已完成（f09b72c）

**Files:**
- Create: `src-tauri/src/commands/storage.rs`
- Modify: `src-tauri/src/commands/mod.rs`（`pub mod storage;`）、`src-tauri/src/lib.rs`（注册 3 条命令）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn 残留数据=plugin_data与注册表差集() {
    let dir = tempfile::tempdir().unwrap();
    let pd = dir.path().join("data").join("plugin-data");
    std::fs::create_dir_all(pd.join("genie-tts")).unwrap();
    std::fs::create_dir_all(pd.join("gone-plugin")).unwrap();
    let installed = ["genie-tts".to_string()];
    let orphans = orphan_plugin_data(&pd, &installed).unwrap();
    assert_eq!(orphans, vec!["gone-plugin".to_string()]);
}

#[test]
fn 未创建目录时报告为空而不是报错() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope");
    assert!(orphan_plugin_data(&missing, &[]).unwrap().is_empty());
}
```

- [ ] **Step 2: 跑测试确认失败**

Expected: `cannot find function \`orphan_plugin_data\` in this scope`。

- [ ] **Step 3: 实现三条命令**

```rust
// 「存储」页与卸载弹窗的后端读口：谁占了多大、哪些是残留、能不能搬家。
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use crate::storage::paths::StoragePaths;
use crate::storage::migrate::{dir_size, legacy_items, migrate_items, MigrationPending};
use crate::plugins::PluginManager;

#[derive(Serialize)]
pub struct StorageBucket { pub kind: String, pub label: String, pub path: String, pub bytes: u64 }

#[derive(Serialize)]
pub struct OrphanData { pub id: String, pub path: String, pub bytes: u64 }

#[derive(Serialize)]
pub struct StorageReport {
    pub config_root: String,
    pub content_root: String,
    pub content_fallback: bool,      // true 时界面必须说明为什么没在安装目录
    pub buckets: Vec<StorageBucket>,
    pub orphans: Vec<OrphanData>,
    pub migration: String,           // "none" | "migratable" | "conflict"
    pub legacy_bytes: u64,
}

/// 只读枚举 plugin-data 里"插件已不在注册表"的残留目录（按名字典序，稳定输出）
pub fn orphan_plugin_data(plugin_data_root: &std::path::Path, installed: &[String])
    -> Result<Vec<String>, String>
{ /* read_dir 失败当空；过滤掉 installed 里的 id；排序返回 */ }

#[tauri::command]
pub fn storage_report(app: AppHandle, paths: State<'_, StoragePaths>, plugins: State<'_, PluginManager>)
    -> Result<StorageReport, String> { /* 汇总 buckets/orphans/migration/legacy_bytes，blocking 线程由调用方保证 */ }

#[tauri::command]
pub async fn delete_plugin_data(id: String, app: AppHandle, paths: State<'_, StoragePaths>)
    -> Result<u64, String>
{ /* spawn_blocking 内删 paths.plugin_data_dir(&id)，返回释放字节数；插件仍安装时报错 */ }

#[tauri::command]
pub async fn migrate_storage_now(app: AppHandle, paths: State<'_, StoragePaths>)
    -> Result<String, String>
{ /* Migratable 才允许；stamp 用 UTC %Y%m%d；返回 "已迁移 N 项 / X MB，原件在 <备份路径>" */ }
```

`lib.rs` 注册：

```rust
            crate::commands::storage::storage_report,
            crate::commands::storage::delete_plugin_data,
            crate::commands::storage::migrate_storage_now,
```

- [ ] **Step 4: 跑测试确认通过**

Expected: `test result: ok. 196 passed; 0 failed; 3 ignored`（+2 例）。

- [ ] **Step 5: 提交**

```powershell
git add src-tauri/src/commands/storage.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(存储): 新增 storage_report/delete_plugin_data/migrate_storage_now 三个读口"
```

**实施记录（与计划的差异）**
- 计划给的是带 `/* */` 的骨架，实现时发现：两根仍相同（未翻默认）时 `migrate_storage_now` 必须能
  拒绝，否则会把数据往自己里拷。判定从 lib.rs 抽成 `migrate::detect_in(paths)`，启动日志与命令共用。
- `MigrationPending` 持有器下线：界面每次读实时状态。缓存快照会在用户搬完家后继续报 Migratable（两处真相）。
- `pointer_json` 之前零调用点：迁移完成后写 `storage.json`（设计 §四 step 4），本 Task 补上。
- 计划预期的测试数（196）已过时，实际 `cargo test --lib` 208 passed / 0 failed。
- 测试夹具不得使用引擎名（`edge-tts`/`genie-tts` 会被基座护栏抓），改用中性 id。

---

### Task 5b: 数据回退急救程序 `va-data-rescue` —— ✅ 已完成（2fa8d5d）

背景：设计文档 §五/§六.3 要求的「从备份回退」与「内容根可改」在本计划里没安排任务（计划范围遗漏）。
2026-10-11 与用户定：只做回退，且不做成设置页按钮而是独立急救程序；不进安装包、不进 Release。

**Files:**
- Create: `src-tauri/src/bin/va-data-rescue.rs`
- Modify: `src-tauri/src/storage/migrate.rs`（抽出 `copy_all_and_verify`，新增 `backup_items`/`restore_conflicts`/`restore_items`/`find_backups`）、`src-tauri/src/storage/paths.rs`（`CONFIG_DIR_NAME`/`POINTER_FILE_NAME`/`config_root_from`）

关键决策：
- 为什么是独立程序：内容根指向的盘没接上时主程序自己都起不来，界面里的按钮点不到。
- 不写第二份复制逻辑：`restore_items` 与 `migrate_items` 共用 `copy_all_and_verify`（设计 §五）。
- **指针指回配置根而不是删掉**：翻默认后删指针 = 内容根退回空的 `exe\data`，刚恢复的数据反而看不见（设计原文已同步修正）。
- 同名条目整批拒绝、备份一律保留、多份备份只取最新但把更早的打印出来。
- 「内容根可改」不做：自动回退 + `VA_STORAGE_DIR` 环境变量已覆盖，1.9.0 不加界面入口。

验收（已跑，不靠“能编译”当“能用”）：
```powershell
cd src-tauri
CARGO_HTTP_CHECK_REVOKE=false; cargo build --bin va-data-rescue
# 假环境：cfg\.migrated-backup-20261011\{messages.json,audio\m_1.wav} + settings.json
target\debug\va-data-rescue.exe --config-root <cfg> --list   # 只列不动
target\debug\va-data-rescue.exe --config-root <cfg> --yes     # 回退 + 写指针，exit=0
target\debug\va-data-rescue.exe --config-root <cfg> --yes     # 再跑：同名拒绝，exit=1
```
实测结果：条目归位、备份保留、`settings.json` 未被触碰、`storage.json` 内容根 == 配置根。

---

### Task 6: 前端「存储」分区与卸载弹窗 —— ✅ 已完成（111f212，真机点击验收待用户）

**Files:**
- Create: `src/components/Settings/StoragePanel.tsx`
- Modify: `src/components/Settings/SettingsPage.tsx`（挂分区）、`src/services/invoke.ts`（4 个封装）、`src/components/Plugins/InstalledCard.tsx`（卸载二选一）、`src/components/Plugins/PluginPage.tsx`（残留分区）

- [ ] **Step 1: 写 invoke 封装与类型**

`src/services/invoke.ts` 新增（与 Rust 侧字段逐一对应，注意命令返回是 snake_case）：

```ts
export interface StorageBucket { kind: string; label: string; path: string; bytes: number }
export interface OrphanData { id: string; path: string; bytes: number }
export interface StorageReport {
  config_root: string; content_root: string; content_fallback: boolean;
  buckets: StorageBucket[]; orphans: OrphanData[];
  migration: "none" | "migratable" | "conflict"; legacy_bytes: number;
}
export async function getStorageReport(): Promise<StorageReport> {
  return invoke<StorageReport>("storage_report");
}
/** 卸载插件。deleteData=false 时保留模型/运行环境（默认） */
export async function uninstallPlugin(id: string, deleteData: boolean): Promise<string> {
  return invoke<string>("uninstall_plugin", { id, deleteData });
}
export async function deletePluginData(id: string): Promise<number> {
  return invoke<number>("delete_plugin_data", { id });
}
export async function migrateStorageNow(): Promise<string> {
  return invoke<string>("migrate_storage_now");
}
```

- [ ] **Step 2: 写 `StoragePanel.tsx`**

要点（不含 TBD）：`useEffect` 拉 `getStorageReport()`；顶部两行显示配置根与内容根路径 + `content_fallback` 时用琥珀色说明「因安装目录不可写，数据存放在此」；`buckets` 表格（label / 体积 / 路径）；`migration !== "none"` 时显示迁移卡（Migratable → 「迁移旧数据（N MB）」按钮调 `migrateStorageNow()` 后重拉；Conflict → 两个按钮 + 取消，不做默认选中）；`orphans` 非空时列表每行「删除数据」并二次确认。体积格式化复用项目里已有的写法（若无则在本文件内实现 `formatBytes(n)` 并加 3 例 vitest：0、1023、GB 级）。

- [ ] **Step 3: 卸载弹窗改二选一**

`InstalledCard.tsx` 卸载按钮的 `ConfirmDialog` 文案改为动态：

```tsx
  const [bytes, setBytes] = useState<number | null>(null);
  // 卸载前先查该插件的重数据体积（失败按 null 处理，不阻塞卸载流程）
  useEffect(() => {
    if (!plugin.has_plugin_data) return;
    void getStorageReport()
      .then((r) => setBytes(r.orphans.find((o) => o.id === plugin.id)?.bytes ?? null))
      .catch(() => setBytes(null));
  }, [plugin.has_plugin_data, plugin.id]);

  const message = bytes
    ? `卸载「${plugin.name}」？\n引擎数据约 ${formatBytes(bytes)}，可选择保留（重装时无需重新下载）。`
    : `卸载「${plugin.name}」？`;
```

两个动作按钮：`保留引擎数据并卸载`（默认）、`卸载并删除引擎数据`；分别调 `uninstallPlugin(id, false / true)`。**不允许**出现"静默默认删数据"。

- [ ] **Step 4: 类型与单测门禁**

```powershell
npx tsc --noEmit                  # 期望零输出
npm run test:run                  # 期望 10 files / 74 tests（新增 formatBytes 3 例）
```

- [ ] **Step 5: 真机检查 + 提交**

`npm run tauri dev` → 设置页「存储」：两根路径与占用正确；插件库卸载 genie 时弹窗显示体积且默认保留；保留后该目录出现在「已下载引擎数据」分区（Task 6 的关键闭环验证，同时验证 Task 3）。

```powershell
git add src/components/Settings/StoragePanel.tsx src/components/Settings/SettingsPage.tsx src/services/invoke.ts src/components/Plugins/InstalledCard.tsx src/components/Plugins/PluginPage.tsx
git commit -m "feat(存储): 设置页新增存储分区，卸载改为显式二选一并显示引擎数据体积"
```

**实施记录（计划有三处会直接写坏，已按实际修正）**
1. Step 3 让卸载弹窗从 `getStorageReport().orphans` 里查体积 —— **这是错的**：`orphans` 的
   定义就是「插件已不在注册表」，卸载当时插件还装着，永远查不到，体积会一直显示未知。
   改为新增按需命令 `plugin_data_size(id)`（不进 `list()`：递归求和可能几万文件，
   插件页一进就全扫会拖住主 runtime）。
2. Step 3 写的文件是 `InstalledCard.tsx` —— 卸载确认实际在 `usePluginActions.ts`（
   `actions.uninstall`），InstalledCard 只负渲染。改对了位置。
3. 卸载要「保留/删除/取消」三态，而项目里的 `confirm()` 只返回 boolean —— 给
   `ConfirmDialog` 加了通用 `choose()`（多选一，Esc/遮罩 = null），而不是复制第二个弹窗壳；
   翻默认后的 Conflict 三选也要用同一个。
4. `formatBytes` 从 VbCableInstallDialog 内部抽到 `utils/format`，并修两处越界（负数 →
   `-Infinity`、TB 级 → `NaN undefined`）。
5. 残留列表抽成 `OrphanDataSection`（渲染+删除一份），插件库与设置-存储两处各自取数、
   都延迟到展开才扫盘（§六.2 与 §六.3 都要用，不写两遍）。
6. `StoragePanel` 的 Conflict 态**不放**「合并」按钮：后端没有对应命令，点了没反应的
   按钮比没有按钮更糟。已把该偏差写回设计文档 §六.3。

实际门禁：`cargo test --lib` 209 passed、`tsc` 零输出、vitest **10 files / 74 tests**、
`npm run build` 通过、三道守卫绿。Step 5 的真机点击尚未做（翻默认前一并交给用户）。

---

### Task 7: 把契约变成可检查的（存储护栏 + 文档）—— ✅ 已完成

**Files:**
- Create: `tools/check-storage-hygiene.ps1`（含中文 → 必须带 BOM）
- Modify: `.github/workflows/consistency.yml`（四步）、`AGENTS.md`（新增「存储契约」）、`C:\Users\XOS\.qoder-cn\skills\voiceassist-dev\SKILL.md` 与 `.claude\skills\voiceassist-dev\SKILL.md`（同步 + 改掉「内置 MiMo」）

- [ ] **Step 1: 写护栏**

与 `check-base-hygiene.ps1` 同形状（`param(-UpdateBaseline)`、`$repoRoot = Split-Path -Parent $PSScriptRoot`、基线文件 `tools/storage-hygiene-baseline.txt` 每行 `路径|模式|次数`、`// storageok:` 本行或紧邻上一行豁免、`plugins/` 不扫、成功显式 `exit 0`）。差异只在两处：

```powershell
# 存储护栏：数据路径只能来自 storage::paths（AGENTS.md「存储契约」的机械化）。
# 只扫 src-tauri/src（前端不碰文件系统路径）。
$patterns = @(
    'app_data_dir\(\)',                       # 除 paths.rs 与 lib.rs 解析处外一律红
    'current_exe\(\)[\s\S]{0,80}join\("(data|plugins)"\)',
    '\.join\("audio"\)|\.join\("voice_samples"\)|\.join\("subtitle_history\.json"\)',
    'LocalDataDir|app_cache_dir'
)
# 基线允许登记的合法位置（解析层自己）：
#   src-tauri/src/storage/paths.rs|app_data_dir()|1
#   src-tauri/src/lib.rs|app_data_dir()|1
```

- [ ] **Step 2: 生基线并逐条核对**

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-storage-hygiene.ps1 -UpdateBaseline
Get-Content .\tools\storage-hygiene-baseline.txt
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-storage-hygiene.ps1
```
Expected: 第二次 `✓ 存储检查通过`。**核对要求**：基线里除 `storage/paths.rs`、`lib.rs` 两处解析点外**不得有其它条目**；出现 `commands/*.rs|app_data_dir()` 之类，说明 Task 2/3 有漏网调用点，回去补，不许登记放宽。

- [ ] **Step 3: 挂 CI**

`.github/workflows/consistency.yml`：注释 `three` → `four`，新增一步（同形状）：

```yaml
      - name: Storage hygiene (data paths must come from storage::paths)
        shell: pwsh
        run: ./tools/check-storage-hygiene.ps1
```

- [ ] **Step 4: 文档回写**

`AGENTS.md` 在「基座合同」后追加：

```markdown
## 存储契约（数据路径只能来自 storage::paths）

1. 业务代码**禁止** `app_data_dir()` / `current_exe()` 拼数据路径，也禁止自己
   `join("audio")` 之类；一律走 `storage::paths::StoragePaths` 的 getter。
2. 配置根（%APPDATA%）只放 settings / 遥控配对 / 存储指针；**其余一律内容根**
   （跟随安装位置）。消息与收藏必须和 `audio/` 同根——删消息按引用计数删音频。
3. 记录里不得写绝对路径（`audio_path`、`clone_voice_path` 保持相对内容根）。
4. `<插件根>` 只放代码（dll/manifest/registry）；模型与运行环境在
   `<内容根>\plugin-data\<id>`，**卸载插件默认不得牵连它**。
5. 执行手段：`pwsh tools/check-storage-hygiene.ps1`（已挂 consistency CI）；
   存量登记在 `tools/storage-hygiene-baseline.txt`，只许变少；行内豁免 `// storageok:`。
```

同步 skill 两份，并把过时的「TTS 引擎：内置 MiMo（小米）、Moss-TTS」改成「内置只剩 Moss-TTS；MiMo 为随包可卸载插件 `mimo-tts`」，插件目录一节补 `VA_STORAGE_DIR` / `VA_PLUGINS_DIR` 与「开发期用它们把多工作树数据对齐，但 dll 版本会跨分支互串，仅作临时手段」。

- [ ] **Step 5: 全量门禁 + 提交**

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-bom.ps1      # 期望 ✓（新 .ps1 带 BOM）
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-base-hygiene.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\check-storage-hygiene.ps1
git add tools/check-storage-hygiene.ps1 tools/storage-hygiene-baseline.txt .github/workflows/consistency.yml AGENTS.md docs/superpowers/plans/2026-10-11-storage-layout.md
git commit -m "chore(存储): 存储护栏上 CI，AGENTS.md 与 skill 增存储契约"
```

**Step 2 实跑结果与校准**：初稿的 `current_exe\(\)[\s\S]{0,80}join\(...\)` 是**伪需求**——
扫描器逐行跑，跨行匹配永远不会命中；改成只抓真违规形态（`\.app_data_dir\(\)` 必须带前导点，
所以 11 处注释/文档提及全部不误伤、只有 lib.rs 一处真调用命中；目录字面量抓 `\.join\("audio"\)` 等）。
`LocalDataDir|app_cache_dir` 拆成两条：模式里带竖线会被护栏自己的基线格式守卫拒掉。
`storage/paths.rs` 不入扫描（它就是路径的唯一主人，扫它只产生无意义基线，同基座护栏不扫 `plugins/`）。

基线 7 条逐条核过：全部是 `#[cfg(test)]` 测脚手架里构造临时目录的字面量（含 migrate.rs 13 处），
无一条是业务代码漏网 —— 按 Step 2 的核对要求，命令层没出现 `app_data_dir()` 即 Task 2/3b 无遗漏。
`lib.rs` 的解析入口用 `// storageok:` 就地豁免而不是占基线额度（该注释**必须写在调用正上方**：
豁免只认本行与紧邻上一行，写在多行说明的第一行会静默失效——实测踩过并已写进 AGENTS.md）。

**反向验证（证明护栏不是摆设）**：临时放一个 `zz_guard_probe.rs` 写三处违规 → 护栏当场红、
退出码 1；删掉后回绿、退出码 0。另：取证时不要用 `Select-Object -First` 接护栏，
它拆管道会让 `$LASTEXITCODE` 停留在上一条命令的值上（差点把“红”读成“绿”）。

---

### Task 8: 收尾（机械改名 + 发布前门禁与真机验收）

- [ ] **Step 1（可选）**：把下游 19 个模块的 `data_dir` 形参机械改名为 `content_dir`，`config_dir` 显式化；每改完一个文件跑一次 `cargo test --lib`，全绿才继续（**纯改名，不许顺手改逻辑**）。
- [ ] **Step 2**：全量门禁 8 道复跑：`cargo test --lib`、`cargo check --lib --tests` 零 warning、`cargo test -p mimo-tts`、`npx tsc --noEmit`、`npm run test:run`、`npm run build`、四道守卫（bom/log/base/storage）、`verify-index -SkipDist`。
- [ ] **Step 3**：真机验收 5 条（设计文档 §九逐条贴结果，尤其「删一条消息仍能按引用删掉音频」与「迁移中途强杀后重跑」）。
- [ ] **Step 4**：回写 `doc/存储布局与迁移设计.md` 状态行为「已实施」+ 本计划勾选与实施记录；`doc/归档/工作线状态.md` 更新。
- [ ] **Step 5**：与 MiMo 线一起走发布（版本号三处同步 1.9.0、release notes、merge main / push / tag / publish.ps1 —— **每一步单独请示用户**）。

---

## 风险与回退（执行中随时对照）

| 风险 | 拦网 |
|---|---|
| 迁移把老用户数据搬坏 | 先复制校验、后移入 `.migrated-backup-*`，永不删除；`migrate_items_with_verify` 注入校验函数并单测「校验失败不动原件」 |
| 安装目录不可写导致数据两处混写 | `decide_content_root` 返回 `fallback` 标记，界面必须显示原因；日志一条 `log_warn!` |
| 有人绕过 paths 自己拼路径 | 存储护栏 + CI；核对时禁止登记放宽 |
| 卸载顺手删掉几 GB 模型 | `uninstall_plugin(delete_data)` 显式参数、UI 默认保留；`sweep_orphan_dirs` 作用域单测锁死 |
| favorites/messages 与 audio 分家 | 设计 §三契约 2 写死同根；真机验收第 2 条专测引用计数 |
| `VA_PLUGINS_DIR` 与内容根混用导致 dll 跨分支互串 | 文档写明仅作开发期临时手段；dev 建议用 `VA_STORAGE_DIR` 指向独立目录 |
