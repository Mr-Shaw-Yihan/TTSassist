// Genie 服务子进程管理：拉起 genie_server.py、健康探活、崩溃后重启。
//
// 进程生命周期：dll 常驻不卸载（插件系统约束），子进程句柄存在全局 OnceLock 里
// 直到主程序退出——与"插件内常驻线程"的既有约定一致。
// 若子进程中途崩溃，ensure_server 的健康探活会发现并重新拉起。

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::paths::Ctx;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Windows: CREATE_NO_WINDOW（GUI 程序拉 python.exe 不弹黑框）
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct ServerState {
    port: u16,
    child: Child,
}

/// 全局服务状态（None = 未启动或已退出）
static SERVER: OnceLock<Mutex<Option<ServerState>>> = OnceLock::new();

fn server_slot() -> &'static Mutex<Option<ServerState>> {
    SERVER.get_or_init(|| Mutex::new(None))
}

/// 确保 Genie 服务在跑，返回可用端口。
/// 已存活直接复用；否则（重新）拉起并等待就绪。
pub fn ensure_server(ctx: &Ctx) -> Result<u16, String> {
    let slot = server_slot();
    let mut guard = slot.lock().map_err(|e| format!("服务状态锁异常: {e}"))?;

    // 已启动且健康 → 直接用
    if let Some(state) = guard.as_ref() {
        if crate::client::health(state.port) {
            return Ok(state.port);
        }
        eprintln!("[genie-tts] 服务进程失联，准备重启");
        if let Some(mut old) = guard.take() {
            let _ = old.child.kill();
            let _ = old.child.wait();
        }
    }

    // 选一个空闲端口
    let port = pick_free_port()?;

    // 日志文件（追加）
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ctx.server_log())
        .map_err(|e| format!("打开服务日志失败: {e}"))?;
    let log_err = log_file.try_clone().map_err(|e| format!("复制日志句柄失败: {e}"))?;

    let mut cmd = Command::new(ctx.python_exe());
    cmd.arg(ctx.server_script())
        .arg("--port")
        .arg(port.to_string())
        .arg("--data-dir")
        .arg(&ctx.data_dir)
        .current_dir(&ctx.data_dir)
        // 强制 UTF-8：genie 源码里有 emoji 输出，GBK 控制台会直接崩
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        // HF 镜像由服务端脚本从 genie-config.json 读取，这里不注入
        .stdin(Stdio::null()) // 杜绝一切 input() 交互可能
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err));
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    eprintln!("[genie-tts] 启动 Genie 服务（端口 {port}）");
    let child = cmd
        .spawn()
        .map_err(|e| format!("启动 Genie 服务失败: {e}"))?;

    // 记下端口号（调试用）
    if let Ok(mut f) = std::fs::File::create(ctx.port_file()) {
        let _ = f.write_all(port.to_string().as_bytes());
    }

    // 等待 /health 就绪（服务端启动只导入 fastapi，不导 genie_tts，通常几秒内）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        if crate::client::health(port) {
            *guard = Some(ServerState { port, child });
            return Ok(port);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // 超时：杀掉残留进程，报错
    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
    Err(format!(
        "Genie 服务启动超时（60 秒未就绪）。可查看日志: {}",
        ctx.server_log().display()
    ))
}

/// 让系统分配一个空闲 TCP 端口
fn pick_free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .map(|l| l.local_addr().map(|a| a.port()).unwrap_or(0))
        .map_err(|e| format!("分配端口失败: {e}"))
        .and_then(|p| {
            if p == 0 {
                Err("分配端口失败".to_string())
            } else {
                Ok(p)
            }
        })
}
