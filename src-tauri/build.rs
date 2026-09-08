fn main() {
    // 知识库默认接入(地址/Key)是编译期注入的:settings.rs 里用 option_env! 读这两个变量。
    // 声明依赖,改了环境变量能触发重新编译,不然会拿着上次编进去的值。
    println!("cargo:rerun-if-env-changed=NEOSERIAL_KB_BASE_URL");
    println!("cargo:rerun-if-env-changed=NEOSERIAL_KB_API_KEY");
    tauri_build::build()
}
