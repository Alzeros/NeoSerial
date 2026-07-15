use crossbeam_channel::{Receiver, Sender};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::sync::{Arc, Mutex};

use crate::buffer::log_line::{Dir, LogLine};
use crate::buffer::ring_buffer::RingBuffer;
use crate::config::settings::{DisplayMode, Settings};
use crate::connection::handle::{ConnCommand, ConnEvent, ConnectionHandle};
use crate::connection::spawn_connection;
use crate::logging::file_logger::FileLogger;
use crate::util::codec::{ascii_to_bytes, hex_to_bytes, LineEnding};

use crate::App;

/// 应用状态：UI 线程持有（经 Arc<Mutex> 共享给各回调闭包）。
struct AppState {
    handle: Option<ConnectionHandle>,
    ring: RingBuffer<LogLine>,
    file_logger: Option<FileLogger>,
    /// 存盘错误接收端（FileLogger 启动时创建，Timer 中消费）。
    log_error_rx: Option<Receiver<String>>,
    error_keywords: Vec<String>,
    serial_defaults: crate::config::settings::SerialDefaults,
    is_hex: bool,
    paused: bool,
    auto_scroll: bool,
    send_history: Vec<String>,
}

/// 后台事件 → UI 线程的中转 channel。
/// （用 channel + Timer 轮询，避免在回调里直接 invoke 的复杂性。）
pub fn run() -> Result<(), slint::PlatformError> {
    let settings = Settings::load();
    let error_keywords = settings.error_keywords.clone();
    let cap = settings.ui.ring_buffer_capacity;

    let app = App::new()?;

    // 应用持久化的窗口尺寸与位置（在初始化其他 UI 状态之前）
    app.window().set_size(slint::PhysicalSize::new(
        settings.window.width,
        settings.window.height,
    ));
    app.window().set_position(slint::PhysicalPosition::new(
        settings.window.x,
        settings.window.y,
    ));

    // 初始化 UI 状态
    let ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.port_name)
        .collect();
    let port_model: VecModel<SharedString> = VecModel::from(
        ports
            .iter()
            .map(|s| SharedString::from(s.as_str()))
            .collect::<Vec<_>>(),
    );
    app.set_ports(ModelRc::new(port_model));
    app.set_port_name(SharedString::from(settings.last_port.as_str()));
    app.set_baud_rate(settings.serial_defaults.baud_rate as i32);
    app.set_ending_index(match settings.ui.line_ending {
        LineEnding::Cr => 0,
        LineEnding::Lf => 1,
        LineEnding::Crlf => 2,
        LineEnding::None => 3,
    });
    app.set_is_hex(matches!(settings.ui.display_mode, DisplayMode::Hex));
    app.set_auto_scroll(settings.ui.auto_scroll);

    // 快捷命令
    let qc_model: VecModel<crate::QuickCmd> = VecModel::from(
        settings
            .quick_commands
            .iter()
            .map(|q| crate::QuickCmd {
                label: q.label.clone().into(),
                content: q.content.clone().into(),
                mode: if matches!(q.mode, DisplayMode::Hex) { 1 } else { 0 },
                ending: match q.line_ending {
                    LineEnding::Cr => 0,
                    LineEnding::Lf => 1,
                    LineEnding::Crlf => 2,
                    LineEnding::None => 3,
                },
            })
            .collect::<Vec<_>>(),
    );
    app.set_quick_commands(ModelRc::new(qc_model));

    let serial_defaults = settings.serial_defaults.clone();
    let state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState {
        handle: None,
        ring: RingBuffer::new(cap),
        file_logger: None,
        log_error_rx: None,
        error_keywords,
        serial_defaults,
        is_hex: matches!(settings.ui.display_mode, DisplayMode::Hex),
        paused: false,
        auto_scroll: settings.ui.auto_scroll,
        send_history: Vec::new(),
    }));

    // 事件 channel：后台 → UI
    let (_event_tx, event_rx): (Sender<ConnEvent>, Receiver<ConnEvent>) =
        crossbeam_channel::unbounded();
    // event_tx 会被各回调 clone 使用；这里用一个新的 sender 句柄保持 channel 活着
    let event_tx = _event_tx.clone();

    // 设置回调
    let weak = app.as_weak();

    // 打开
    {
        let weak = weak.clone();
        let state = state.clone();
        let event_tx = event_tx.clone();
        app.on_open_clicked(move || {
            let app = weak.unwrap();
            let port_name = app.get_port_name().to_string();
            let baud = app.get_baud_rate() as u32;
            let mut st = state.lock().unwrap();
            if st.handle.is_some() {
                return;
            }
            let kws = st.error_keywords.clone();
            let sd = st.serial_defaults.clone();
            // 若存盘已开启，把 log sender clone 传入读线程（连接打开时快照）
            let log_tx = st.file_logger.as_ref().map(|fl| fl.log_sender());
            match spawn_connection(
                &port_name,
                baud,
                sd.data_bits,
                sd.parity,
                sd.stop_bits,
                sd.flow_control,
                kws,
                event_tx.clone(),
                log_tx,
            ) {
                Ok(h) => {
                    st.handle = Some(h);
                    app.set_connected(true);
                    app.set_status_text("已连接".into());
                    app.set_status_color(slint::Color::from_rgb_u8(0x7e, 0xe7, 0x87).into());
                }
                Err(e) => {
                    app.set_status_text(format!("打开失败: {}", e).into());
                    app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72).into());
                }
            }
        });
    }

    // 关闭
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_close_clicked(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            if let Some(h) = st.handle.take() {
                h.send_command(ConnCommand::SetLogger(None));
                h.send_command(ConnCommand::Close);
            }
            if let Some(fl) = st.file_logger.take() {
                fl.stop();
            }
            st.log_error_rx = None;
            app.set_connected(false);
            app.set_status_text("未连接".into());
            app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72).into());
        });
    }

    // 暂停
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_pause_toggled(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.paused = app.get_paused();
        });
    }

    // 清屏
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_clear_clicked(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.ring.clear();
            // 清空 model
            let m = app.get_log_model();
            if let Some(vm) = m.as_any().downcast_ref::<VecModel<crate::LogLine>>() {
                vm.set_vec(Vec::new());
            }
        });
    }

    // 显示模式
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_mode_changed(move |is_hex| {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.is_hex = is_hex;
            // 重渲染所有行的 text 字段
            refresh_model(&app, &st);
        });
    }

    // 自动滚动
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_auto_scroll_toggled(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.auto_scroll = app.get_auto_scroll();
        });
    }

    // 存盘开关（MVP：用固定路径）
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_logging_toggled(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            if st.file_logger.is_none() {
                let path = std::env::temp_dir().join("neoserial_capture.bin");
                let (error_tx, error_rx) = crossbeam_channel::unbounded::<String>();
                match FileLogger::start(path.clone(), error_tx) {
                    Ok(fl) => {
                        // 连接已打开时，通过 SetLogger 把 sender 传给读线程（动态启停存盘）
                        if let Some(h) = &st.handle {
                            h.send_command(ConnCommand::SetLogger(Some(fl.log_sender())));
                        }
                        st.log_error_rx = Some(error_rx);
                        st.file_logger = Some(fl);
                        app.set_logging(true);
                        app.set_log_file_name(path.to_string_lossy().to_string().into());
                    }
                    Err(e) => {
                        app.set_status_text(format!("存盘失败: {}", e).into());
                        app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72).into());
                    }
                }
            } else {
                // 关闭存盘：通知读线程停止存盘
                if let Some(h) = &st.handle {
                    h.send_command(ConnCommand::SetLogger(None));
                }
                if let Some(fl) = st.file_logger.take() {
                    fl.stop();
                }
                st.log_error_rx = None;
                app.set_logging(false);
                app.set_log_file_name("".into());
            }
        });
    }

    // 发送
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_send(move |text, mode, ending| {
            let app = weak.unwrap();
            let text = text.to_string();
            let mut st = state.lock().unwrap();
            if st.handle.is_none() {
                app.set_error_hint("未连接".into());
                return;
            }
            let bytes_result = if mode == 1 {
                hex_to_bytes(&text)
            } else {
                Ok(ascii_to_bytes(&text))
            };
            let mut bytes = match bytes_result {
                Ok(b) => {
                    app.set_error_hint("".into());
                    b
                }
                Err(e) => {
                    app.set_error_hint(e.into());
                    return;
                }
            };
            let le = match ending {
                0 => LineEnding::Cr,
                1 => LineEnding::Lf,
                2 => LineEnding::Crlf,
                _ => LineEnding::None,
            };
            le.append_bytes(&mut bytes);

            // 推历史
            st.send_history.push(text.clone());
            if st.send_history.len() > 50 {
                st.send_history.remove(0);
            }

            if let Some(h) = &st.handle {
                h.send_command(ConnCommand::Send(bytes));
            }
            // 发送后输入框保留（不清空）
        });
    }

    // port-selected / baud-changed / add-quick-cmd：MVP 暂留空实现（端口刷新靠重启）
    {
        let weak = weak.clone();
        app.on_port_selected(move |p| {
            let app = weak.unwrap();
            app.set_port_name(p);
        });
    }
    {
        let weak = weak.clone();
        app.on_baud_changed(move |b| {
            let app = weak.unwrap();
            app.set_baud_rate(b);
        });
    }
    {
        let weak = weak.clone();
        app.on_add_quick_cmd(move || {
            let _app = weak.unwrap();
            // MVP：快捷命令编辑留 v2，此处仅占位
        });
    }

    // 历史上翻：取 send_history 最后一条填入输入框（MVP 简版，无 cursor）
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_history_prev(move || {
            let app = weak.unwrap();
            let st = state.lock().unwrap();
            if let Some(last) = st.send_history.last() {
                app.set_input_text(last.clone().into());
            }
        });
    }
    // 历史下翻：MVP 简版，清空输入框
    {
        let weak = weak.clone();
        app.on_history_next(move || {
            let app = weak.unwrap();
            app.set_input_text("".into());
        });
    }

    // 20ms Timer：拉事件 → 刷新 UI
    let timer_weak = weak.clone();
    let timer_state = state.clone();
    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(20),
        move || {
            let app = timer_weak.unwrap();
            let mut st = timer_state.lock().unwrap();
            // 存盘错误处理（不受暂停影响——存盘错误需立即通知用户）
            // 先 take 出 rx 以避免借用冲突
            if let Some(rx) = st.log_error_rx.take() {
                let mut error_msg: Option<String> = None;
                while let Ok(msg) = rx.try_recv() {
                    error_msg = Some(msg);
                }
                if let Some(msg) = error_msg {
                    // 存盘失败：关闭存盘开关 + 红字提示（设计 §7.2）
                    if let Some(h) = &st.handle {
                        h.send_command(ConnCommand::SetLogger(None));
                    }
                    if let Some(fl) = st.file_logger.take() {
                        fl.stop();
                    }
                    app.set_logging(false);
                    app.set_log_file_name("".into());
                    app.set_status_text(msg.into());
                    app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72).into());
                } else {
                    // 无错误，把 rx 放回
                    st.log_error_rx = Some(rx);
                }
            }
            // 取所有待处理事件。
            // 暂停 = 只冻结屏幕，后台照收照丢旧（设计 §4.3）：
            //   - Lines 照常 push 进环形缓冲（不论是否 paused），照常丢旧；
            //   - 仅在非 paused 时 refresh_model 刷屏——恢复后能看到暂停期间进缓冲的数据。
            //   - Disconnected/SendFailed 仍照常处理（断开/发送失败需立即通知用户）。
            let mut new_lines: Vec<LogLine> = Vec::new();
            let mut disconnected: Option<String> = None;
            let mut send_failed: Option<String> = None;
            while let Ok(ev) = event_rx.try_recv() {
                match ev {
                    // 不论是否 paused 都收集：后台照收，环形缓冲照常丢旧
                    ConnEvent::Lines(lines) => new_lines.extend(lines),
                    ConnEvent::Disconnected(msg) => disconnected = Some(msg),
                    ConnEvent::SendFailed(msg) => send_failed = Some(msg),
                }
            }
            if let Some(msg) = disconnected {
                st.handle = None;
                app.set_connected(false);
                app.set_status_text(msg.into());
                app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72).into());
            }
            if let Some(msg) = send_failed {
                app.set_error_hint(msg.into());
            }
            if !new_lines.is_empty() {
                // 照常进缓冲（后台照收，照常丢旧）—— 不论是否 paused
                st.ring.push_many(new_lines);
                // 只在不 paused 时刷屏：暂停冻结屏幕，恢复后即可看到暂停期间的数据
                if !st.paused {
                    refresh_model(&app, &st);
                }
            }
        },
    );
    // 保活 timer
    std::mem::forget(timer);

    // 每 2s 刷新端口列表（USB 热插拔）
    let port_weak = weak.clone();
    let port_timer = Timer::default();
    port_timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_secs(2),
        move || {
            let app = port_weak.unwrap();
            let ports: Vec<SharedString> = serialport::available_ports()
                .unwrap_or_default()
                .into_iter()
                .map(|p| SharedString::from(p.port_name))
                .collect();
            let m = app.get_ports();
            if let Some(vm) = m.as_any().downcast_ref::<VecModel<SharedString>>() {
                vm.set_vec(ports);
            }
        },
    );
    // 保活 port_timer
    std::mem::forget(port_timer);

    // 窗口关闭时保存配置 + 清理连接/存盘
    // 闭包不能持有强引用 App，故用 close_weak 在闭包内 upgrade。
    let close_state = state.clone();
    let close_weak = weak.clone();
    app.window().on_close_requested(move || {
        if let Some(app) = close_weak.upgrade() {
            let mut st = close_state.lock().unwrap();
            // 全量写：先 load 保留其余字段，仅更新本会话改过的项
            let mut settings = Settings::load();
            settings.last_port = app.get_port_name().to_string();
            settings.ui.line_ending = match app.get_ending_index() {
                0 => LineEnding::Cr,
                1 => LineEnding::Lf,
                2 => LineEnding::Crlf,
                _ => LineEnding::None,
            };
            settings.ui.display_mode = if st.is_hex { DisplayMode::Hex } else { DisplayMode::Ascii };
            settings.ui.auto_scroll = st.auto_scroll;
            // 保存窗口尺寸与位置（物理像素）
            let size = app.window().size();
            let pos = app.window().position();
            settings.window.width = size.width;
            settings.window.height = size.height;
            settings.window.x = pos.x;
            settings.window.y = pos.y;
            // 保存波特率（Slint int 属性为 i32，转 u32）
            settings.serial_defaults.baud_rate = app.get_baud_rate() as u32;
            // 关闭连接：发 Close，读线程随后退出
            if let Some(h) = st.handle.as_ref() {
                // 先停止读线程的存盘（避免向已 stop 的 FileLogger send）
                h.send_command(ConnCommand::SetLogger(None));
                h.send_command(ConnCommand::Close);
            }
            // 停存盘：stop 消费 self，需 take 取出所有权
            if let Some(fl) = st.file_logger.take() {
                fl.stop();
            }
            let _ = settings.save();
        }
        slint::CloseRequestResponse::HideWindow
    });

    app.run()
}

/// 根据环形缓冲 + 当前显示模式，全量重建 UI model。
/// 5000 行全量重建在 20ms 内可接受；后续可优化为增量。
fn refresh_model(app: &App, st: &AppState) {
    let snapshot = st.ring.snapshot();
    let view: Vec<crate::LogLine> = snapshot
        .iter()
        .map(|l| {
            let text = if st.is_hex { &l.hex } else { &l.ascii };
            let color = if l.is_error {
                slint::Color::from_rgb_u8(0xff, 0x7b, 0x72)
            } else {
                match l.dir {
                    Dir::Tx => slint::Color::from_rgb_u8(0x7e, 0xe7, 0x87),
                    Dir::Rx => slint::Color::from_rgb_u8(0xc8, 0xd3, 0xe0),
                }
            };
            crate::LogLine {
                ts: l.ts.clone().into(),
                text: text.clone().into(),
                color: color.into(),
                dir: if matches!(l.dir, Dir::Tx) { 1 } else { 0 },
            }
        })
        .collect();
    let m = app.get_log_model();
    if let Some(vm) = m.as_any().downcast_ref::<VecModel<crate::LogLine>>() {
        vm.set_vec(view);
    } else {
        let new_model = VecModel::from(view);
        app.set_log_model(ModelRc::new(new_model));
    }
}
