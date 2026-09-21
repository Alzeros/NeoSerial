//! 数据处理 command。帧构造逻辑全在 crate::dataproc::frame(纯函数),
//! 这里只做 rng 注入与错误透传;将来 MCP frame_build 工具复用同一实现。

use crate::dataproc::frame::{build_frame as build_frame_impl, FrameBuildError, FrameConfig, FrameResult};

#[tauri::command]
pub fn build_frame(config: FrameConfig) -> Result<FrameResult, FrameBuildError> {
    let mut rng = rand::rng();
    build_frame_impl(&config, &mut rng)
}
