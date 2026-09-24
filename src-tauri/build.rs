fn main() {
    // 知识库默认接入(地址/Key)是编译期注入的:settings.rs 里用 option_env! 读这两个变量。
    // 声明依赖,改了环境变量能触发重新编译,不然会拿着上次编进去的值。
    println!("cargo:rerun-if-env-changed=NEOSERIAL_KB_BASE_URL");
    println!("cargo:rerun-if-env-changed=NEOSERIAL_KB_API_KEY");
    // DPI 模式必须在创建任何 HWND 前确定。默认构建写入系统 DPI 的 exe manifest，
    // 避免应用启动后再调用 SetProcessDpiAwareness 被框架已设的模式拒绝。
    println!("cargo:rerun-if-changed=windows-system-dpi.manifest");
    let mut windows = tauri_build::WindowsAttributes::new();
    if cfg!(feature = "system-dpi") {
        windows = windows.app_manifest(include_str!("windows-system-dpi.manifest"));
    }
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("Tauri build failed");
}
