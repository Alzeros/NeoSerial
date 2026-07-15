use crossbeam_channel::{bounded, Sender};

use crate::config::settings::{DataBits, FlowControl, Parity, StopBits};
use crate::connection::handle::{ConnCommand, ConnEvent, ConnectionHandle};

pub mod handle;
pub mod line_assembler;
pub mod reader;
pub mod writer;

/// 打开串口并 spawn 读线程。成功返回 ConnectionHandle。
/// log_tx 可选：Some 时读线程把原始字节直接 send 给存盘线程（设计 §4.1）。
pub fn spawn_connection(
    port_name: &str,
    baud: u32,
    data_bits: DataBits,
    parity: Parity,
    stop_bits: StopBits,
    flow_control: FlowControl,
    error_keywords: Vec<String>,
    event_tx: Sender<ConnEvent>,
    log_tx: Option<Sender<Vec<u8>>>,
) -> std::io::Result<ConnectionHandle> {
    let port = serialport::new(port_name, baud)
        .data_bits(map_data_bits(data_bits))
        .parity(map_parity(parity))
        .stop_bits(map_stop_bits(stop_bits))
        .flow_control(map_flow_control(flow_control))
        .open()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let (cmd_tx, cmd_rx) = bounded::<ConnCommand>(64);

    std::thread::Builder::new()
        .name(format!("neoserial-reader-{}", port_name))
        .spawn(move || {
            reader::reader_loop(port, cmd_rx, event_tx, error_keywords, log_tx);
        })?;

    Ok(ConnectionHandle { cmd_tx })
}

fn map_data_bits(d: DataBits) -> serialport::DataBits {
    match d {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    }
}

fn map_parity(p: Parity) -> serialport::Parity {
    match p {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    }
}

fn map_stop_bits(s: StopBits) -> serialport::StopBits {
    match s {
        StopBits::One => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    }
}

fn map_flow_control(f: FlowControl) -> serialport::FlowControl {
    match f {
        FlowControl::None => serialport::FlowControl::None,
        FlowControl::Software => serialport::FlowControl::Software,
        FlowControl::Hardware => serialport::FlowControl::Hardware,
    }
}
