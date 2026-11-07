//! A native observer that keeps working when renderer JavaScript stops executing.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

struct Renderer {
    expected: bool,
    since: Instant,
    heartbeat: Option<Instant>,
    session: String,
    visible: bool,
    reported: bool,
    window_events: HashMap<&'static str, Value>,
}
impl Renderer {
    fn new(now: Instant) -> Self {
        Self {
            expected: true,
            since: now,
            heartbeat: None,
            session: String::new(),
            visible: true,
            reported: false,
            window_events: HashMap::new(),
        }
    }
    fn stalled(&mut self, now: Instant) -> Option<Value> {
        let gap = now.duration_since(self.heartbeat.unwrap_or(self.since));
        if self.expected && !self.reported && self.visible && gap >= Duration::from_secs(10) {
            self.reported = true;
            return Some(
                json!({"session": self.session, "gap_ms": gap.as_millis(), "never_started": self.heartbeat.is_none(), "last_known_visible": self.visible,
                "interpretation": "页面未及时上报；可能为主线程阻塞、引擎故障、IPC 阻塞或系统调度，需结合其他记录"}),
            );
        }
        None
    }
}
static RENDERERS: LazyLock<Mutex<HashMap<String, Renderer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn page_load(window: &str, stage: &str, expected: bool) {
    if let Ok(mut windows) = RENDERERS.lock() {
        if stage == "Started" {
            windows.insert(window.to_string(), Renderer::new(Instant::now()));
        }
        windows
            .entry(window.to_string())
            .or_insert_with(|| Renderer::new(Instant::now()))
            .expected = expected;
    }
}
pub fn expects_heartbeat(window: &str) -> bool {
    RENDERERS
        .lock()
        .ok()
        .and_then(|windows| windows.get(window).map(|state| state.expected))
        .unwrap_or(false)
}
pub fn closed(window: &str) {
    if let Ok(mut windows) = RENDERERS.lock() {
        windows.remove(window);
    }
}

pub fn window_event(window: &str, event: &tauri::WindowEvent) {
    let (kind, data) = match event {
        tauri::WindowEvent::Focused(focused) => ("focus", json!({"focused":focused})),
        tauri::WindowEvent::Moved(position) => ("position", json!({"x":position.x,"y":position.y})),
        tauri::WindowEvent::Resized(size) => {
            ("size", json!({"width":size.width,"height":size.height}))
        }
        tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            ("scale", json!({"scale":scale_factor}))
        }
        _ => return,
    };
    if let Ok(mut windows) = RENDERERS.lock() {
        let state = windows.entry(window.to_string()).or_insert_with(|| {
            let mut state = Renderer::new(Instant::now());
            state.expected = false;
            state
        });
        state.window_events.insert(kind, data);
    }
}

pub fn start(app: &tauri::AppHandle) {
    let app = app.clone();
    super::frontend::native_event(
        "application".into(),
        "runtime.started",
        json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"version":env!("CARGO_PKG_VERSION"),"data_mode":crate::config::dir_mode()}),
    );
    tauri::async_runtime::spawn(async move {
        let mut previous = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let now = Instant::now();
            #[cfg(any(windows, test))]
            super::cdp::flush();
            let scheduler_gap = now.duration_since(previous);
            previous = now;
            if scheduler_gap > Duration::from_secs(6) {
                super::frontend::native_event(
                    "application".into(),
                    "runtime.monitor_delay",
                    json!({"gap_ms": scheduler_gap.as_millis(), "note":"原生监控也被延迟，可能涉及系统休眠/调度"}),
                );
            }
            let mut snapshots = Vec::new();
            if let Ok(mut windows) = RENDERERS.lock() {
                for (window, renderer) in windows.iter_mut() {
                    if let Some(stall) = renderer.stalled(now) {
                        snapshots.push(window.clone());
                        super::frontend::native_event(
                            window.clone(),
                            "renderer.heartbeat_missing",
                            stall,
                        );
                    }
                    if !renderer.window_events.is_empty() {
                        if !snapshots.contains(window) {
                            snapshots.push(window.clone());
                        }
                        super::frontend::native_event(
                            window.clone(),
                            "window.native_state",
                            json!({"latest": std::mem::take(&mut renderer.window_events)}),
                        );
                    }
                }
            }
            for label in snapshots {
                if let Some(window) = app.get_webview_window(&label) {
                    super::window::snapshot(window, None, "native_lifecycle");
                }
            }
        }
    });
}

#[tauri::command]
pub async fn frontend_diagnostics_heartbeat(
    webview_window: tauri::WebviewWindow,
    session: String,
    visibility: String,
) -> Result<(), String> {
    if session.len() > 100 || !matches!(visibility.as_str(), "visible" | "hidden") {
        return Err("invalid renderer heartbeat".into());
    }
    let mut windows = RENDERERS.lock().map_err(|error| error.to_string())?;
    let now = Instant::now();
    let state = windows
        .entry(webview_window.label().to_string())
        .or_insert_with(|| Renderer::new(now));
    if state.reported {
        super::frontend::native_event(
            webview_window.label().to_string(),
            "renderer.heartbeat_resumed",
            json!({"previous_session":state.session,"session":session,"gap_ms":now.duration_since(state.heartbeat.unwrap_or(state.since)).as_millis()}),
        );
    }
    state.heartbeat = Some(now);
    state.session = session;
    state.visible = visibility == "visible";
    state.reported = false;
    state.expected = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_new_unknown_window_that_never_started_and_recovers_without_spamming() {
        let start = Instant::now();
        let mut renderer = Renderer::new(start);
        assert!(renderer.stalled(start + Duration::from_secs(9)).is_none());
        assert_eq!(
            renderer.stalled(start + Duration::from_secs(11)).unwrap()["never_started"],
            true
        );
        assert!(renderer.stalled(start + Duration::from_secs(30)).is_none());
        renderer.heartbeat = Some(start + Duration::from_secs(30));
        renderer.reported = false;
        assert!(renderer.stalled(start + Duration::from_secs(35)).is_none());
        assert_eq!(
            renderer.stalled(start + Duration::from_secs(41)).unwrap()["never_started"],
            false
        );
    }
    #[test]
    fn hidden_window_timer_throttling_is_not_reported_as_a_visible_renderer_stall() {
        let start = Instant::now();
        let mut renderer = Renderer::new(start);
        renderer.visible = false;
        assert!(renderer.stalled(start + Duration::from_secs(120)).is_none());
    }

    #[test]
    fn native_keepalive_or_external_page_without_an_injected_recorder_is_not_a_failed_app_page() {
        let start = Instant::now();
        let mut renderer = Renderer::new(start);
        renderer.expected = false;
        assert!(renderer.stalled(start + Duration::from_secs(120)).is_none());
    }
}
