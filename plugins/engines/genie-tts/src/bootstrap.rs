// Python 运行时引导：下载 embeddable Python → pip → jieba_fast wheel → genie-tts。
//
// 全部步骤幂等：每步先探测是否已完成，已完成直接跳过。
//
// 关键设计（踩过坑）：
// - jieba_fast 在 PyPI 只有源码包（无 wheel），内嵌 Python 无头文件编译必失败
//   → 本 crate 内嵌预编译好的 cp312 wheel（include_bytes!），优先安装它，
//   之后 pip 装 genie-tts 时该依赖已满足，不再触发源码编译
// - Python 版本固定 3.12.10（与 wheel 的 cp312 标签匹配）；EMBED_VERSION 标记
//   不一致时整个 python 目录重装（处理旧版本残留）
// - cb 为进度回调（可空）：percent 0~100 定量 / <0 不定量 + 文案
//
// 注意：不要求用户机器装过 Python——本插件自带一份独立的 embeddable Python，
// 与系统环境完全隔离。

use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::paths::{Ctx, GenieConfig};

/// Windows: CREATE_NO_WINDOW（GUI 程序拉 python 不弹黑框）。
/// 注意：has_module 等频繁探测的子进程也必须带此标志，
/// 否则每次合成兜底链路都会闪控制台窗口。
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 构造 python 子进程命令（Windows 自动附带无控制台窗口标志）
fn python_command(python: &Path) -> Command {
    let mut cmd = Command::new(python);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// 进度回调类型（percent<0 表示不定量，以文案为准）
pub type ProgressCb<'a> = Option<&'a dyn Fn(f32, &str)>;

/// 内嵌 Python 版本（与 jieba_fast wheel 的 cp312 标签匹配）
pub const PY_VERSION: &str = "3.12.10";
/// embeddable 版下载地址（python.org 官方，国内可直连）
const PY_EMBED_URL: &str =
    "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip";
/// get-pip 引导脚本（embeddable 版默认不带 pip）
const GET_PIP_URL: &str = "https://bootstrap.pypa.io/get-pip.py";

/// 内嵌的服务端脚本源码（随 dll 分发，运行期写到数据目录）
const SERVER_SCRIPT: &str = include_str!("../server/genie_server.py");

/// 预编译的 jieba_fast wheel（PyPI 无 wheel，见文件头说明）
const JIEBA_WHEEL: &[u8] =
    include_bytes!("../wheels/jieba_fast-0.53-cp312-cp312-win_amd64.whl");
const JIEBA_WHEEL_NAME: &str = "jieba_fast-0.53-cp312-cp312-win_amd64.whl";

fn report(cb: ProgressCb, percent: f32, msg: &str) {
    if let Some(f) = cb {
        f(percent, msg);
    }
}

/// 确保 Python 运行时 + jieba_fast + genie-tts 就绪（幂等）。
/// 进度区间约定：本函数占用整体进度的 0~40。
pub fn ensure_python_runtime(ctx: &Ctx, cfg: &GenieConfig, cb: ProgressCb) -> Result<(), String> {
    // 1. Python 解释器（含版本迁移：标记不符 → 删掉重装）
    let marker = ctx.python_dir().join("EMBED_VERSION");
    if ctx.python_exe().exists() {
        let installed = std::fs::read_to_string(&marker).unwrap_or_default();
        if installed.trim() != PY_VERSION {
            report(cb, -1.0, "检测到运行环境版本更新，正在重新安装 Python…");
            if let Err(e) = std::fs::remove_dir_all(ctx.python_dir()) {
                return Err(format!(
                    "清理旧版 Python 运行时失败（请关闭应用后重试）: {e}"
                ));
            }
        }
    }
    if !ctx.python_exe().exists() {
        install_embedded_python(ctx, cb)?;
        let _ = std::fs::write(&marker, PY_VERSION);
    }
    let python = ctx.python_exe();

    // 2. pip（embeddable 默认没有，用 get-pip.py 装）
    if !has_module(&python, "pip") {
        report(cb, 10.0, "正在安装 pip…");
        bootstrap_pip(ctx, &python)?;
    }

    // 3. jieba_fast（内嵌 wheel 直装，避免源码编译）
    if !has_module(&python, "jieba_fast") {
        report(cb, 14.0, "正在安装中文分词组件…");
        install_jieba_wheel(ctx, &python)?;
    }

    // 4. genie-tts 库（含 onnxruntime 等，约 200MB，耗时最长）
    if !has_module(&python, "genie_tts") {
        report(cb, -1.0, "正在安装语音合成依赖（约 200MB，请耐心等待）…");
        install_genie(ctx, &python, cfg)?;
    }

    // 5. 服务端脚本（每次都覆写，保证与 dll 版本一致）
    std::fs::write(ctx.server_script(), SERVER_SCRIPT)
        .map_err(|e| format!("写入服务端脚本失败: {e}"))?;

    report(cb, 40.0, "运行环境就绪");
    Ok(())
}

/// 探测某 Python 模块是否存在（importlib.util.find_spec，不真正 import——
/// 真正 import genie_tts 会在资源缺失时走 input() 交互，子进程里会挂）
fn has_module(python: &Path, module: &str) -> bool {
    let script = format!(
        "import importlib.util,sys; sys.exit(0 if importlib.util.find_spec('{module}') else 1)"
    );
    python_command(python)
        .args(["-c", &script])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 下载并解压 embeddable Python，修补 ._pth 允许 site-packages
fn install_embedded_python(ctx: &Ctx, cb: ProgressCb) -> Result<(), String> {
    let dl_dir = ctx.dl_dir();
    std::fs::create_dir_all(&dl_dir).map_err(|e| format!("创建下载目录失败: {e}"))?;
    let zip_path = dl_dir.join("python-embed.zip");

    report(cb, 0.0, "正在下载 Python 运行时（约 11MB）…");
    // 下载按字节映射到 0~9 进度段
    let stage_cb = |done: u64, total: Option<u64>| {
        if let Some(f) = cb {
            match total {
                Some(t) if t > 0 => {
                    let pct = (done as f64 / t as f64 * 9.0).min(9.0) as f32;
                    f(pct, "正在下载 Python 运行时…")
                }
                _ => f(-1.0, "正在下载 Python 运行时…"),
            }
        }
    };
    crate::client::download_file_with_progress(PY_EMBED_URL, &zip_path, Some(&stage_cb))?;

    report(cb, 9.0, "正在解压 Python 运行时…");
    let target = ctx.python_dir();
    std::fs::create_dir_all(&target).map_err(|e| format!("创建 Python 目录失败: {e}"))?;
    crate::util::extract_zip(&zip_path, &target)?;
    let _ = std::fs::remove_file(&zip_path);

    // embeddable 版用 pythonXXX._pth 限制 import 路径，默认不含 site-packages；
    // 取消 "import site" 的注释才能让 pip 装的包被找到
    patch_pth(&target)?;

    if !ctx.python_exe().exists() {
        return Err("Python 运行时解压后未找到 python.exe，请重试".into());
    }
    Ok(())
}

/// 修补 ._pth：确保 "import site" 一行存在且未被注释
fn patch_pth(python_dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(python_dir)
        .map_err(|e| format!("读取 Python 目录失败: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with("._pth") {
            let path = entry.path();
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("读取 {name} 失败: {e}"))?;
            let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
            // 把注释掉的 import site 放开；没有就追加
            let mut found = false;
            for line in lines.iter_mut() {
                let t = line.trim();
                if t == "#import site" || t == "# import site" {
                    *line = "import site".to_string();
                    found = true;
                } else if t == "import site" {
                    found = true;
                }
            }
            if !found {
                lines.push("import site".to_string());
            }
            std::fs::write(&path, lines.join("\n"))
                .map_err(|e| format!("写入 {name} 失败: {e}"))?;
            return Ok(());
        }
    }
    Err("未找到 ._pth 配置文件（embeddable Python 不完整）".into())
}

/// 用 get-pip.py 给 embeddable Python 装 pip
fn bootstrap_pip(ctx: &Ctx, python: &Path) -> Result<(), String> {
    let dl_dir = ctx.dl_dir();
    std::fs::create_dir_all(&dl_dir).map_err(|e| format!("创建下载目录失败: {e}"))?;
    let get_pip = dl_dir.join("get-pip.py");

    crate::client::download_file_with_progress(GET_PIP_URL, &get_pip, None)?;

    let output = python_command(python)
        .arg(&get_pip)
        .arg("--no-warn-script-location")
        .current_dir(&ctx.data_dir)
        .output()
        .map_err(|e| format!("运行 get-pip 失败: {e}"))?;
    if !output.status.success() {
        return Err(format!("pip 安装失败: {}", tail_stderr(&output.stderr)));
    }
    let _ = std::fs::remove_file(&get_pip);
    Ok(())
}

/// 安装内嵌的 jieba_fast wheel（PyPI 上该包无 wheel，源码编译需要 C 环境，
/// 用户机器不可依赖——见文件头说明）
fn install_jieba_wheel(ctx: &Ctx, python: &Path) -> Result<(), String> {
    let dl_dir = ctx.dl_dir();
    std::fs::create_dir_all(&dl_dir).map_err(|e| format!("创建下载目录失败: {e}"))?;
    let wheel_path = dl_dir.join(JIEBA_WHEEL_NAME);
    std::fs::write(&wheel_path, JIEBA_WHEEL)
        .map_err(|e| format!("释放内置组件失败: {e}"))?;

    let output = python_command(python)
        .args(["-m", "pip", "install", "--no-warn-script-location"])
        .arg(&wheel_path)
        .current_dir(&ctx.data_dir)
        .output()
        .map_err(|e| format!("运行 pip 失败: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "中文分词组件安装失败: {}",
            tail_stderr(&output.stderr)
        ));
    }
    Ok(())
}

/// pip 安装 genie-tts（jieba_fast 已由内嵌 wheel 满足，不会触发源码编译）
fn install_genie(ctx: &Ctx, python: &Path, cfg: &GenieConfig) -> Result<(), String> {
    let mut cmd = python_command(python);
    cmd.args(["-m", "pip", "install", "--no-warn-script-location"]);
    if !cfg.pip_index_url.trim().is_empty() {
        cmd.args(["-i", cfg.pip_index_url.trim()]);
    }
    cmd.arg("genie-tts").current_dir(&ctx.data_dir);

    let output = cmd.output().map_err(|e| format!("运行 pip 失败: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "genie-tts 安装失败（可尝试在 genie-config.json 更换 pip_index_url 后重试）: {}",
            tail_stderr(&output.stderr)
        ));
    }
    Ok(())
}

/// 取 stderr 末尾若干字节作为错误摘要（避免超长报错刷屏）
fn tail_stderr(stderr: &[u8]) -> String {
    let s = String::from_utf8_lossy(stderr);
    let trimmed = s.trim();
    if trimmed.len() <= 600 {
        trimmed.to_string()
    } else {
        trimmed[trimmed.len() - 600..].to_string()
    }
}
