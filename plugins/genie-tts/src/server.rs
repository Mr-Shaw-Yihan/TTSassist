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

    // 把子进程挂到带 KILL_ON_JOB_CLOSE 的 Job Object：宿主进程退出时
    // （句柄随进程关闭）系统自动杀掉服务进程，避免孤儿进程长期占用内存。
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::io::AsRawHandle;
        win_job::attach_kill_on_close(child.as_raw_handle() as *mut std::ffi::c_void);
    }

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

/// 当前在跑的服务端口（未启动/失联返回 None）。不拉起进程，仅探活。
/// 音色卸载时用它判断是否需要先让服务端卸载内存中的音色。
pub fn running_port() -> Option<u16> {
    let guard = server_slot().lock().ok()?;
    let state = guard.as_ref()?;
    if crate::client::health(state.port) {
        Some(state.port)
    } else {
        None
    }
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

// ── Windows Job Object：子进程随宿主退出 ──────────────
//
// 插件 dll 常驻不卸载、无 drop 时机，没法在"应用退出"时主动杀子进程。
// 用 Job Object + JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE：job 句柄存在静态槽里
// 直到宿主进程退出，进程退出时 OS 关闭全部句柄 → job 关闭 → 系统自动
// 终止 job 内所有进程。全程无需 Rust 侧 drop，天然防孤儿。
#[cfg(target_os = "windows")]
mod win_job {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    type Handle = *mut c_void;

    #[repr(C)]
    struct JobObjectBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_object_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    struct JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

    extern "system" {
        fn CreateJobObjectW(job_attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(
            job: Handle,
            info_class: u32,
            info: *const JobObjectExtendedLimitInformation,
            info_len: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    }

    /// 把进程挂到"关闭即杀"的 Job Object。失败静默（尽力而为，不阻塞主流程）。
    pub fn attach_kill_on_close(process_handle: Handle) {
        // 裸指针包一层以满足 static 的 Send+Sync 要求（句柄仅本模块使用）
        struct JobHandle(Handle);
        unsafe impl Send for JobHandle {}
        unsafe impl Sync for JobHandle {}

        static JOB: OnceLock<JobHandle> = OnceLock::new();
        let job = JOB.get_or_init(|| unsafe {
            let h = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if h.is_null() {
                return JobHandle(std::ptr::null_mut());
            }
            let mut info: JobObjectExtendedLimitInformation = std::mem::zeroed();
            info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                h,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                &info,
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            );
            if ok == 0 {
                return JobHandle(std::ptr::null_mut());
            }
            // 故意不关闭句柄：随宿主进程退出时 OS 回收 → 触发 KILL_ON_JOB_CLOSE
            JobHandle(h)
        });
        if job.0.is_null() {
            return;
        }
        unsafe {
            let _ = AssignProcessToJobObject(job.0, process_handle);
        }
    }
}
