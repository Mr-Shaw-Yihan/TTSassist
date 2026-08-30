// Python 运行时引导：下载 embeddable Python → pip → 推理依赖 + 服务端脚本。
//
// 全部步骤幂等：每步先探测是否已完成，已完成直接跳过。
//
// 依赖清单说明：不装上游 requirements.txt 全家桶（torch+cu128/transformers/
// optimum 是 GPU 训练向的，Windows PyPI 的 torch 本就是 CPU 版）。只装
// onnx_model.py 实际 import 的包：numpy / onnxruntime / soundfile / torch /
// librosa / scipy / tokenizers / onnx（模型是 bfloat16，CPU 推理靠 onnx 包
// 把权重提升为 fp32，缺了会加载失败）+ fastapi / uvicorn / huggingface_hub
// （服务端与模型下载用）。
//
// 注意：不要求用户机器装过 Python——本插件自带一份独立的 embeddable Python，
// 与系统环境完全隔离。

use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::paths::Ctx;

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

/// 内嵌 Python 版本
pub const PY_VERSION: &str = "3.12.10";
/// embeddable 版下载地址（python.org 官方，国内可直连）
const PY_EMBED_URL: &str =
    "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip";
/// get-pip 引导脚本（embeddable 版默认不带 pip）
const GET_PIP_URL: &str = "https://bootstrap.pypa.io/get-pip.py";

/// 推理依赖清单（升级依赖时改这里并同步 DEPS_VERSION，触发重装）
const DEPS: &[&str] = &[
    "fastapi",
    "uvicorn",
    "huggingface_hub",
    "numpy",
    "soundfile",
    "tokenizers",
    "onnxruntime",
    "onnx",
    "torch",
    "librosa",
    "scipy",
];
/// 依赖清单版本标记（内容变化时递增，标记不符 → 重装依赖）；
/// setup.rs 的磁盘探测也读它，保持 crate 内可见
pub(crate) const DEPS_VERSION: &str = "1";

/// 内嵌的服务端脚本源码（随 dll 分发，运行期写到数据目录）
const SERVER_SCRIPT: &str = include_str!("../server/hojo_server.py");
/// 上游推理模块（Apache-2.0，原样内嵌）
const ONNX_MODEL_SCRIPT: &str = include_str!("../server/onnx_model.py");
/// 上游许可全文（Apache-2.0 要求随分发附带）
const UPSTREAM_LICENSE: &str = include_str!("../server/LICENSE-Hojo-Apache-2.0.txt");

fn report(cb: ProgressCb, percent: f32, msg: &str) {
    if let Some(f) = cb {
        f(percent, msg);
    }
}

/// 确保 Python 运行时 + 推理依赖 + 服务端脚本就绪（幂等）。
/// 进度区间约定：本函数占用整体进度的 0~40。
pub fn ensure_python_runtime(ctx: &Ctx, cb: ProgressCb) -> Result<(), String> {
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

    // 3. 推理依赖（torch+onnxruntime+librosa 等，约 1.2GB，耗时最长）
    let deps_marker = ctx.python_dir().join("DEPS_VERSION");
    let deps_installed = std::fs::read_to_string(&deps_marker).unwrap_or_default();
    if deps_installed.trim() != DEPS_VERSION {
        report(cb, -1.0, "正在安装语音合成依赖（约 1.2GB，请耐心等待）…");
        install_deps(ctx, &python)?;
        let _ = std::fs::write(&deps_marker, DEPS_VERSION);
    }

    // 4. 服务端脚本 + 上游推理模块 + 许可文本（每次都覆写，保证与 dll 版本一致）
    write_server_scripts(ctx)?;

    report(cb, 40.0, "运行环境就绪");
    Ok(())
}

/// 探测某 Python 模块是否存在（importlib.util.find_spec，不真正 import）
pub fn has_module(python: &Path, module: &str) -> bool {
    let script = format!(
        "import importlib.util,sys; sys.exit(0 if importlib.util.find_spec('{module}') else 1)"
    );
    python_command(python)
        .args(["-c", &script])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 把内嵌的服务端脚本与上游文件写到数据目录
fn write_server_scripts(ctx: &Ctx) -> Result<(), String> {
    std::fs::write(ctx.server_script(), SERVER_SCRIPT)
        .map_err(|e| format!("写入服务端脚本失败: {e}"))?;
    std::fs::write(ctx.onnx_model_script(), ONNX_MODEL_SCRIPT)
        .map_err(|e| format!("写入推理模块失败: {e}"))?;
    let _ = std::fs::write(
        ctx.data_dir.join("LICENSE-Hojo-Apache-2.0.txt"),
        UPSTREAM_LICENSE,
    );
    Ok(())
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

/// pip 安装推理依赖：多安装源回退（面向小白无配置，单源被网络/安全软件掐断时自动换源）。
/// 环境变量 HOJO_TTS_PIP_INDEX_URL 可指定唯一源（排障后门，指定后不再回退）。
fn install_deps(ctx: &Ctx, python: &Path) -> Result<(), String> {
    const DEFAULT_INDEXES: &[&str] = &[
        "https://pypi.tuna.tsinghua.edu.cn/simple",
        "https://mirrors.cloud.tencent.com/pypi/simple",
        "https://pypi.org/simple",
    ];
    let indexes: Vec<String> = match std::env::var(crate::paths::ENV_PIP_INDEX) {
        Ok(v) if !v.trim().is_empty() => vec![v.trim().to_string()],
        _ => DEFAULT_INDEXES.iter().map(|s| s.to_string()).collect(),
    };

    let mut last_err = String::new();
    for index in &indexes {
        let mut cmd = python_command(python);
        cmd.args(["-m", "pip", "install", "--no-warn-script-location", "-i", index])
            .args(DEPS)
            .current_dir(&ctx.data_dir);
        let output = cmd.output().map_err(|e| format!("运行 pip 失败: {e}"))?;
        if output.status.success() {
            return Ok(());
        }
        last_err = format!("（安装源 {index}）{}", tail_stderr(&output.stderr));
    }
    Err(format!("推理依赖安装失败（已尝试全部安装源）: {last_err}"))
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
