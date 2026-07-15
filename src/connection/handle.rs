use crossbeam_channel::Sender;

/// UI → 后台线程的命令。
pub enum ConnCommand {
    /// 发送字节。
    Send(Vec<u8>),
    /// 关闭连接（读线程会随后退出）。
    Close,
    /// 更新存盘 sender。Some=开启/切换存盘，None=关闭存盘。
    /// 允许连接已打开时动态启停存盘，存盘字节从读线程直发。
    SetLogger(Option<Sender<Vec<u8>>>),
}

/// 后台线程 → UI 的事件。
pub enum ConnEvent {
    /// 新收到的若干完整行。
    Lines(Vec<crate::buffer::log_line::LogLine>),
    /// 连接断开（含原因）。
    Disconnected(String),
    /// 发送失败。
    SendFailed(String),
}

/// UI 线程持有的连接句柄。通过它向后台线程下达命令。
pub struct ConnectionHandle {
    pub(crate) cmd_tx: Sender<ConnCommand>,
}

impl ConnectionHandle {
    pub fn send_command(&self, cmd: ConnCommand) {
        let _ = self.cmd_tx.send(cmd);
    }
}
