fn main() {
    slint_build::compile("ui/app.slint").unwrap();

    // Windows：把 .ico 嵌入 exe，资源管理器 / 任务栏快捷方式会显示
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/NeoSerial.ico");
        // 可选元数据，任务管理器/属性面板可见
        res.set("ProductName", "NeoSerial");
        res.set("FileDescription", "NeoSerial 串口调试工具");
        if let Err(e) = res.compile() {
            // 缺 RC 工具时不阻断构建，只警告
            println!("cargo:warning=winres 嵌入图标失败: {e}");
        }
    }

    println!("cargo:rerun-if-changed=assets/NeoSerial.ico");
    println!("cargo:rerun-if-changed=assets/NeoSerial.png");
    println!("cargo:rerun-if-changed=ui/app.slint");
}
