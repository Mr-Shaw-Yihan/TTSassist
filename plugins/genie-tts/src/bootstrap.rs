// Python 运行时引导：下载 embeddable Python → 装 pip → 装 genie-tts → 写服务端脚本。
//
// 全部步骤幂等：每步先探测是否已完成，已完成直接跳过。首次完整引导约需
// 下载 ~250MB（Python ~11MB + pip + genie-tts 依赖含 onnxruntime ~200MB），
// 之后 GenieData 与音色由服务端脚本负责下载（见 server/genie_server.py）。
//
// 注意：不要求用户机器装过 Python——本插件自带一份独立的 embeddable Python，
// 与系统环境完全隔离。

use std::path::Path;
use std::process::Command;

use crate::paths::{Ctx, GenieConfig};

/// embeddable 版下载地址（python.org 官方，国内可直连；genie-tts 要求 Python ≥3.9）
const PY_EMBED_URL: &str =
    "https://www.python.org/ftp/python/3.11.9/python-3.11.9-embed-amd64.zip";
/// get-pip 引导脚本（embeddable 版默认不带 pip）
const GET_PIP_URL: &str = "https://bootstrap.pypa.io/get-pip.py";

/// 内嵌的服务端脚本源码（随 dll 分发，运行期写到数据目录）
const SERVER_SCRIPT: &str = include_str!("../server/genie_server.py");

/// 确保 Python 运行时 + genie-tts 就绪（幂等）。返回可用的 python.exe 路径。
pub fn ensure_python_runtime(ctx: &Ctx, cfg: &GenieConfig) -> Result<(), String> {
    // 1. Python 解释器
    if !ctx.python_exe().exists() {
        install_embedded_python(ctx)?;
    }
    let python = ctx.python_exe();

    // 2. pip（embeddable 默认没有，用 get-pip.py 装）
    if !has_pip(&python) {
        bootstrap_pip(ctx, &python)?;
    }

    // 3. genie-tts 库
    if !has_genie(&python) {
        install_genie(ctx, &python, cfg)?;
    }

    // 4. 服务端脚本（每次都覆写，保证与 dll 版本一致）
    std::fs::write(ctx.server_script(), SERVER_SCRIPT)
        .map_err(|e| format!("写入服务端脚本失败: {e}"))?;

    Ok(())
}

/// 探测 `python -m pip` 是否可用
fn has_pip(python: &Path) -> bool {
    Command::new(python)
        .args(["-m", "pip", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 探测 genie_tts 是否可导入（不触发资源检查——仅 importlib 探测模块存在性）
fn has_genie(python: &Path) -> bool {
    // 用 importlib.util.find_spec 只查模块是否存在，不真正 import
    // （真正 import 会在资源缺失时走 input() 交互，子进程里会挂）
    Command::new(python)
        .args([
            "-c",
            "import importlib.util,sys; sys.exit(0 if importlib.util.find_spec('genie_tts') else 1)",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 下载并解压 embeddable Python，修补 ._pth 允许 site-packages
fn install_embedded_python(ctx: &Ctx) -> Result<(), String> {
    let dl_dir = ctx.dl_dir();
    std::fs::create_dir_all(&dl_dir).map_err(|e| format!("创建下载目录失败: {e}"))?;
    let zip_path = dl_dir.join("python-embed.zip");

    eprintln!("[genie-tts] 下载 Python 运行时（首次，约 11MB）…");
    crate::client::download_file(PY_EMBED_URL, &zip_path)?;

    let target = ctx.python_dir();
    std::fs::create_dir_all(&target).map_err(|e| format!("创建 Python 目录失败: {e}"))?;
    crate::util::extract_zip(&zip_path, &target)?;
    let _ = std::fs::remove_file(&zip_path);

    // embeddable 版用 python311._pth 限制 import 路径，默认不含 site-packages；
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

    eprintln!("[genie-tts] 下载 pip 引导脚本…");
    crate::client::download_file(GET_PIP_URL, &get_pip)?;

    eprintln!("[genie-tts] 安装 pip…");
    let output = Command::new(python)
        .arg(&get_pip)
        .arg("--no-warn-script-location")
        .current_dir(&ctx.data_dir)
        .output()
        .map_err(|e| format!("运行 get-pip 失败: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "pip 安装失败: {}",
            tail_stderr(&output.stderr)
        ));
    }
    let _ = std::fs::remove_file(&get_pip);
    Ok(())
}

/// pip 安装 genie-tts（含 onnxruntime 等依赖，约 200MB，耗时较长）
fn install_genie(ctx: &Ctx, python: &Path, cfg: &GenieConfig) -> Result<(), String> {
    eprintln!("[genie-tts] 安装 genie-tts 依赖（首次约 200MB，请耐心等待）…");
    let mut cmd = Command::new(python);
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
