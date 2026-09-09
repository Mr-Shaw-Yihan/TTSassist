// 子进程派生 helper。
//
// 背景：本体是 GUI 程序（没有控制台），一旦派生 netsh / powershell 这类控制台程序，
// Windows 会临时给它分配一个控制台窗口——即用户看到的「点一下遥控就闪一下黑框」。
// 所有外部命令派生统一走 hidden_command，避免每个调用点各自记得加 flag。

/// 构造一个不会闪控制台窗口的 Command（Windows 下追加 CREATE_NO_WINDOW）。
pub fn hidden_command(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
