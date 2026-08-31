// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // dev 构建用独立的 WebView2 数据目录：dev 与已安装 release 的 identifier 相同,
    // 会共用 %LOCALAPPDATA%\com.neoserial.app\EBWebView,两版先后/并发使用同一目录
    // 会出现浏览器进程锁冲突,release 启动即白屏。必须在任何窗口创建之前设置。
    // 代价仅是 dev 首次启动数据目录全新(本应用设置在后端 settings.json,不受影响)。
    #[cfg(debug_assertions)]
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        std::env::set_var(
            "WEBVIEW2_USER_DATA_FOLDER",
            format!("{}\\com.neoserial.app.dev", base),
        );
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    neoserial_lib::run()
}
