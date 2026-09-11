// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    // WebView2 数据目录必须在任何窗口创建之前设好。两种覆盖,便携优先:
    // ① 便携模式 / NEOSERIAL_DATA_DIR:让 webview 的缓存也跟着数据走,便携版不往系统盘
    //    写这几十 MB(否则"解压即用"仍在 %LOCALAPPDATA% 留一大坨)。
    // ② dev 构建:dev 与已安装 release 的 identifier 相同,会共用
    //    %LOCALAPPDATA%\com.neoserial.app\EBWebView,两版先后/并发使用同一目录会出现
    //    浏览器进程锁冲突,release 启动即白屏。代价仅是 dev 首次启动数据目录全新
    //    (本应用设置在后端 settings.json,不受影响)。
    if let Some(dir) = neoserial_lib::webview2_data_dir() {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", dir);
    } else {
        #[cfg(debug_assertions)]
        if let Some(dir) = neoserial_lib::webview_default_parent() {
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", dir);
        }
    }

    neoserial_lib::run()
}
