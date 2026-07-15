use crossbeam_channel::Sender;
use crate::util::log_format::DisplayConfig;

/// UI → 后台线程的命令。
pub enum ConnCommand {
    /// 发送字节。
    Send(Vec<u8>),
    /// 关闭连接。
    Close,
    /// 更新存盘 sender。Some=开启存盘，None=关闭。
    SetLogger(Option<Sender<String>>),
    /// 更新显示配置（时间戳/记录发送开关），运行中可动态改。
    SetDisplay(DisplayConfig),
}

/// 后台线程 → UI 的事件。
pub enum ConnEvent {
    /// 新收到的若干完整行（已按 log_send 过滤：log_send=false 时不含 TX 行）。
    Lines(Vec<crate::buffer::log_line::LogLine>),
    /// 连接断开。
    Disconnected(String),
    /// 发送失败。
    SendFailed(String),
    /// 收发字节统计（累计值，周期上报）。
    Stats { tx_bytes: u64, rx_bytes: u64 },
}

pub struct ConnectionHandle {
    pub(crate) cmd_tx: Sender<ConnCommand>,
}

impl ConnectionHandle {
    pub fn send_command(&self, cmd: ConnCommand) {
        let _ = self.cmd_tx.send(cmd);
    }
}
