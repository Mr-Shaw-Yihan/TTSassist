fn main() {
    // 关闭 tauri_build 的 RC 方式 app manifest（其内容仅 Common-Controls v6 依赖），
    // 改由链接器统一嵌入（下方 link-arg）——两种方式同用时 RC 资源与链接器生成的
    // manifest 都是资源 ID 1，会 CVT1100 duplicate resource 链接失败。
    let attrs = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attrs).expect("tauri_build 失败");

    // 全部二进制嵌入 Common-Controls v6 依赖 manifest（与 tauri 默认 app manifest 等价）。
    // 背景：测试 exe 链接进来的窗口/托盘代码（tao/muda/rfd 经 windows crate）引用
    // TaskDialogIndirect / SetWindowSubclass，这些导出只在 v6 公共控件里；测试 exe
    // 原先没有 manifest → 加载即 STATUS_ENTRYPOINT_NOT_FOUND（cargo test 全挂）。
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
         name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
         publicKeyToken='6595b64144ccf1df' language='*' processorArchitecture='*'"
    );
}
