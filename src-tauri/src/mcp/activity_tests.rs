use super::activity::{McpActivity, RECENT_PORT_LIMIT};

#[test]
fn start_finish_preserve_concurrent_calls_and_original_port() {
    let mut activity = McpActivity::default();
    let first = activity.start("send_and_read", Some("COM1".into()), 1000);
    let second = activity.start("send_file", Some("COM2".into()), 1100);
    let started = activity.snapshot();
    assert_eq!(started.active.len(), 2);
    activity.finish(second, true, 2000);
    let finished = activity.snapshot();
    assert!(finished.revision > started.revision);
    assert_eq!(finished.active[0].id, first);
    assert_eq!(finished.recent[0].port.as_deref(), Some("COM2"));
    assert_eq!(finished.recent[0].ok, Some(true));
    activity.finish(first, false, 3000);
    assert!(activity.snapshot().active.is_empty());
    assert_eq!(activity.snapshot().recent[0].ok, Some(false));
}

#[test]
fn clear_removes_recent_but_does_not_cancel_active_or_reuse_ids() {
    let mut activity = McpActivity::default();
    let first = activity.start("send", Some("COM1".into()), 1000);
    activity.finish(first, true, 2000);
    let active = activity.start("send_and_read", Some("COM1".into()), 3000);
    let revision = activity.snapshot().revision;
    activity.clear_recent();
    assert!(activity.snapshot().recent.is_empty());
    assert_eq!(activity.snapshot().active[0].id, active);
    assert!(activity.snapshot().revision > revision);
    activity.finish(active, false, 4000);
    let next = activity.start("send", None, 5000);
    assert!(next > active);
}

#[test]
fn recent_keeps_only_latest_per_scope_and_is_bounded() {
    let mut activity = McpActivity::default();
    for index in 0..(RECENT_PORT_LIMIT + 10) {
        let call = activity.start("get_status", Some(format!("COM{}", index)), 1000);
        activity.finish(call, true, 2000);
    }
    assert_eq!(activity.snapshot().recent.len(), RECENT_PORT_LIMIT);
    let call = activity.start("send", Some("COM20".into()), 3000);
    activity.finish(call, true, 4000);
    let snapshot = activity.snapshot();
    assert_eq!(snapshot.recent.len(), RECENT_PORT_LIMIT);
    assert_eq!(
        snapshot
            .recent
            .iter()
            .filter(|call| call.port.as_deref() == Some("COM20"))
            .count(),
        1
    );
    assert_eq!(snapshot.recent[0].tool, "send");
}
