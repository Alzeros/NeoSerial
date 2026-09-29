//! 数据处理 command。帧构造逻辑全在 crate::dataproc::frame(纯函数),
//! 这里只做 rng 注入与错误透传;将来 MCP frame_build 工具复用同一实现。

use crate::dataproc::frame::{build_frame as build_frame_impl, FrameBuildError, FrameConfig, FrameResult};

#[tauri::command]
pub fn build_frame(config: FrameConfig) -> Result<FrameResult, FrameBuildError> {
    let mut rng = rand::rng();
    build_frame_impl(&config, &mut rng)
}

/// 浏览器只提供 GBK 解码；编码交给原生层。批量处理转义文本的普通文字片段，
/// 避免每个片段发起一次 IPC。在线程池执行，长文本不阻塞窗口事件。
#[tauri::command]
pub async fn encode_gbk_texts(texts: Vec<String>) -> Result<Vec<Vec<u8>>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        texts.iter().map(|text| crate::util::codec::text_to_gbk(text)).collect()
    })
    .await
    .map_err(|e| format!("GBK 编码任务执行异常: {}", e))?
}
