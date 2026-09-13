// 性能埋点命令（T2）：前端 main 窗口首帧回执 startup 指标。
// 起点在 lib.rs run() 第一行写入 perf::START，终点是前端本命令调用。

/// 前端 main 窗口首帧调用：记录 run() 进入 → 前端首帧总耗时（只记第一次）。
#[tauri::command]
pub fn perf_startup_done() {
    crate::perf::startup_done();
}
