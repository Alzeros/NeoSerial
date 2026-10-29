use std::panic::{catch_unwind, AssertUnwindSafe};

use serde_json::json;

use super::{parse_tool_call, CallRecorder};
use crate::mcp::state::test_support::test_shared;

#[test]
fn recorder_tracks_start_and_finish_without_changing_existing_log_record() {
    let shared = test_shared();
    let mut first = CallRecorder::new(
        shared.clone(),
        "send_and_read".into(),
        &json!({"port":"COM1"}),
    );
    let second = CallRecorder::new(shared.clone(), "send".into(), &json!({"port":"COM2"}));
    assert_eq!(
        shared
            .call_log
            .lock()
            .unwrap()
            .activity
            .snapshot()
            .active
            .len(),
        2
    );
    first.set_result(true, String::new());
    drop(first);
    let snapshot = shared.call_log.lock().unwrap().activity.snapshot();
    assert_eq!(snapshot.active.len(), 1);
    assert_eq!(snapshot.active[0].port.as_deref(), Some("COM2"));
    assert_eq!(snapshot.recent[0].ok, Some(true));
    drop(second);
    let log = shared.call_log.lock().unwrap();
    assert!(log.activity.snapshot().active.is_empty());
    assert_eq!(log.snapshot().len(), 2);
    assert!(!log.snapshot()[1].ok);
}

#[test]
fn malformed_arguments_and_unknown_tools_do_not_leave_active_entries() {
    let shared = test_shared();
    for (tool, args) in [
        ("send", json!({"port":"COM1"})),
        ("unknown_tool", json!({"port":"COM2"})),
    ] {
        let mut recorder = CallRecorder::new(shared.clone(), tool.into(), &args);
        assert!(parse_tool_call(tool, args, &mut recorder).is_err());
        drop(recorder);
        assert!(shared
            .call_log
            .lock()
            .unwrap()
            .activity
            .snapshot()
            .active
            .is_empty());
    }
    let snapshot = shared.call_log.lock().unwrap().activity.snapshot();
    assert_eq!(snapshot.recent[0].port, None);
    assert_eq!(snapshot.recent[0].ok, Some(false));
}

#[test]
fn clear_during_a_call_and_unwinding_preserve_cleanup() {
    let shared = test_shared();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let _recorder =
            CallRecorder::new(shared.clone(), "send_file".into(), &json!({"port":"COM1"}));
        shared.call_log.lock().unwrap().clear();
        assert_eq!(
            shared
                .call_log
                .lock()
                .unwrap()
                .activity
                .snapshot()
                .active
                .len(),
            1
        );
        panic!("simulated call unwind");
    }));
    assert!(outcome.is_err());
    let snapshot = shared.call_log.lock().unwrap().activity.snapshot();
    assert!(snapshot.active.is_empty());
    assert_eq!(snapshot.recent[0].ok, Some(false));
}

#[test]
fn application_tools_ignore_a_spurious_port_argument() {
    let shared = test_shared();
    let recorder = CallRecorder::new(
        shared.clone(),
        "get_settings".into(),
        &json!({"port":"COM1"}),
    );
    assert_eq!(
        shared.call_log.lock().unwrap().activity.snapshot().active[0].port,
        None
    );
    drop(recorder);
}
