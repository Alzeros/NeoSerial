fn main() {
    // 知识库默认接入(地址/Key)是编译期注入的:settings.rs 里用 option_env! 读这两个变量。
    // 声明依赖,改了环境变量能触发重新编译,不然会拿着上次编进去的值。
    println!("cargo:rerun-if-env-changed=NEOSERIAL_KB_BASE_URL");
    println!("cargo:rerun-if-env-changed=NEOSERIAL_KB_API_KEY");
    // 图标会嵌入原生程序；只改图片时也要重新生成窗口图标和 Windows EXE 资源。
    println!("cargo:rerun-if-changed=icons");
    // DPI 模式必须在创建任何 HWND 前确定。默认构建写入系统 DPI 的 exe manifest，
    // 避免应用启动后再调用 SetProcessDpiAwareness 被框架已设的模式拒绝。
    println!("cargo:rerun-if-changed=windows-system-dpi.manifest");
    println!("cargo:rerun-if-changed=windows-common-controls.manifest");
    let mut windows = tauri_build::WindowsAttributes::new();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        // tauri-build 只把资源链接到 bin，cargo test --lib 的测试程序拿不到清单，
        // 加载 TaskDialogIndirect 时会在任何测试执行前以 0xc0000139 退出。
        // 改由链接器为所有可执行目标嵌入清单，同时关闭 Tauri 的重复清单资源。
        // https://github.com/tauri-apps/tauri/issues/13419
        let manifest_file = if cfg!(feature = "system-dpi") {
            "windows-system-dpi.manifest"
        } else {
            "windows-common-controls.manifest"
        };
        let manifest = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is required"),
        )
        .join(manifest_file);
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    } else if cfg!(feature = "system-dpi") {
        windows = windows.app_manifest(include_str!("windows-system-dpi.manifest"));
    }
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("Tauri build failed");
}
