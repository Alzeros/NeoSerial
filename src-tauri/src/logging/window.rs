//! Native observations shared by every current and future application window.
use serde_json::{json, Value};

pub fn stage(window: &str, request_id: Option<u64>, stage: &'static str, extra: Value) {
    crate::logging::frontend::native_event(
        window.to_string(),
        "settings.native",
        json!({
            "stage": stage, "request_id": request_id, "detail": extra,
        }),
    );
}

fn intersects(a: (i32, i32, u32, u32), b: (i32, i32, u32, u32)) -> bool {
    let (ax, ay, aw, ah) = (a.0 as i64, a.1 as i64, a.2 as i64, a.3 as i64);
    let (bx, by, bw, bh) = (b.0 as i64, b.1 as i64, b.2 as i64, b.3 as i64);
    aw > 0
        && ah > 0
        && bw > 0
        && bh > 0
        && ax < bx + bw
        && ax + aw > bx
        && ay < by + bh
        && ay + ah > by
}

/// Sample on a worker so diagnostic OS queries never delay the opening handshake.
pub fn snapshot(window: tauri::WebviewWindow, request_id: Option<u64>, reason: &'static str) {
    super::frontend::native_event(
        window.label().to_string(),
        "window.snapshot_requested",
        json!({ "reason": reason, "request_id":request_id }),
    );
    let requested_at = chrono::Local::now().to_rfc3339();
    tauri::async_runtime::spawn_blocking(move || {
        let position = window.outer_position();
        let size = window.outer_size();
        let monitors = window.available_monitors();
        let intersects_monitor = match (&position, &size, &monitors) {
            (Ok(p), Ok(s), Ok(monitors)) => Some(monitors.iter().any(|monitor| {
                let mp = monitor.position();
                let ms = monitor.size();
                intersects(
                    (p.x, p.y, s.width, s.height),
                    (mp.x, mp.y, ms.width, ms.height),
                )
            })),
            _ => None,
        };
        super::frontend::native_event(
            window.label().to_string(),
            "window.snapshot",
            json!({
                "request_id":request_id,
                "reason": reason, "requested_at": requested_at, "sampled_at": chrono::Local::now().to_rfc3339(),
                "visible": window.is_visible().map_err(|error| error.to_string()),
                "minimized": window.is_minimized().map_err(|error| error.to_string()),
                "focused": window.is_focused().map_err(|error| error.to_string()),
                "scale": window.scale_factor().map_err(|error| error.to_string()),
                "position": position.map(|p| json!({"x": p.x, "y": p.y})).map_err(|error| error.to_string()),
                "size": size.map(|s| json!({"width": s.width, "height": s.height})).map_err(|error| error.to_string()),
                "intersects_monitor": intersects_monitor,
                "monitors": monitors.map(|monitors| monitors.into_iter().map(|monitor| {
                    let p = monitor.position(); let s = monitor.size();
                    json!({"x": p.x, "y": p.y, "width": s.width, "height": s.height, "scale": monitor.scale_factor()})
                }).collect::<Vec<_>>()).map_err(|error| error.to_string()),
            }),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_offscreen_and_zero_size_without_rejecting_negative_monitor_coordinates() {
        assert!(intersects((-1500, 100, 680, 500), (-1920, 0, 1920, 1080)));
        assert!(!intersects((-1500, 100, 680, 500), (0, 0, 1920, 1080)));
        assert!(!intersects((1920, 0, 680, 500), (0, 0, 1920, 1080)));
        assert!(!intersects((100, 100, 0, 500), (0, 0, 1920, 1080)));
        assert!(intersects((1919, 100, 680, 500), (0, 0, 1920, 1080)));
    }
}
