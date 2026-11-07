//! Native WebView2 events are independent of page JavaScript and apply to every window.
#[cfg(windows)]
mod windows_hooks {
    use super::super::frontend::native_event;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use webview2_com::{
        take_pwstr, BrowserProcessExitedEventHandler, CallDevToolsProtocolMethodCompletedHandler,
        DevToolsProtocolEventReceivedEventHandler, NavigationCompletedEventHandler,
        ProcessFailedEventHandler,
    };
    use windows_core::{Interface, PWSTR};

    struct Hooks {
        core: ICoreWebView2,
        process_token: i64,
        browser: Option<(ICoreWebView2Environment5, i64)>,
        navigation: Option<i64>,
        frame_navigation: Option<i64>,
        browser_logs: Vec<(ICoreWebView2DevToolsProtocolEventReceiver, i64)>,
    }
    impl Drop for Hooks {
        fn drop(&mut self) {
            unsafe {
                let _ = self.core.remove_ProcessFailed(self.process_token);
                if let Some(token) = self.navigation {
                    let _ = self.core.remove_NavigationCompleted(token);
                }
                if let Some(token) = self.frame_navigation {
                    let _ = self.core.remove_FrameNavigationCompleted(token);
                }
                for (receiver, token) in &self.browser_logs {
                    let _ = receiver.remove_DevToolsProtocolEventReceived(*token);
                }
                if let Some((environment, token)) = &self.browser {
                    let _ = environment.remove_BrowserProcessExited(*token);
                }
            }
        }
    }
    thread_local! { static HOOKS: RefCell<HashMap<String, Hooks>> = RefCell::new(HashMap::new()); }

    fn kind_name(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> &'static str {
        match kind {
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => "browser_process_exited",
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => "renderer_process_exited",
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => "renderer_unresponsive",
            COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED => "frame_renderer_exited",
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => "gpu_process_exited",
            COREWEBVIEW2_PROCESS_FAILED_KIND_UTILITY_PROCESS_EXITED => "utility_process_exited",
            _ => "other_process_exited",
        }
    }
    fn navigation(
        label: &str,
        scope: &str,
        args: Option<ICoreWebView2NavigationCompletedEventArgs>,
    ) {
        if let Some(args) = args {
            unsafe {
                let mut success = windows_core::BOOL(0);
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                let mut id = 0;
                let mut http = 0;
                let success_ok = args.IsSuccess(&mut success).is_ok();
                let status_ok = args.WebErrorStatus(&mut status).is_ok();
                let id_ok = args.NavigationId(&mut id).is_ok();
                let http_ok = args
                    .cast::<ICoreWebView2NavigationCompletedEventArgs2>()
                    .ok()
                    .is_some_and(|args| args.HttpStatusCode(&mut http).is_ok());
                native_event(
                    label.to_string(),
                    "webview.navigation_completed",
                    json!({"scope":scope,"success":success_ok.then_some(success.0!=0),"web_error":status_ok.then_some(status.0),"navigation_id":id_ok.then_some(id),"http_status":http_ok.then_some(http)}),
                );
            }
        }
    }
    pub fn install(webview: &tauri::Webview) {
        let label = webview.label().to_string();
        let dispatch_label = label.clone();
        if let Err(error) = webview.with_webview(move |platform| unsafe {
            if HOOKS.with(|hooks| hooks.borrow().contains_key(&label)) { return; }
            let result = (|| -> windows_core::Result<()> {
                let core = platform.controller().CoreWebView2()?;
                let environment = platform.environment();
                let mut version = PWSTR::null();
                let version = environment.BrowserVersionString(&mut version).ok().map(|_| take_pwstr(version));
                let mut reports = PWSTR::null();
                let failure_report_folder = environment.cast::<ICoreWebView2Environment11>().ok()
                    .and_then(|environment| environment.FailureReportFolderPath(&mut reports).ok()).map(|_| take_pwstr(reports));
                let failure_profile = json!({"browser_version":version,"failure_report_folder":failure_report_folder});
                let process_label = label.clone();
                let process_handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        let kind_ok = args.ProcessFailedKind(&mut kind).is_ok();
                        let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                        let mut exit_code = 0;
                        let detail = args.cast::<ICoreWebView2ProcessFailedEventArgs2>().ok();
                        let reason_ok = detail.as_ref().is_some_and(|detail| detail.Reason(&mut reason).is_ok());
                        let exit_ok = detail.as_ref().is_some_and(|detail| detail.ExitCode(&mut exit_code).is_ok());
                        native_event(process_label.clone(), "webview.process_failed", json!({
                            "kind": kind_ok.then_some(kind.0), "kind_name": if kind_ok {kind_name(kind)} else {"unavailable"},
                            "reason_code": reason_ok.then_some(reason.0), "exit_code": exit_ok.then_some(exit_code),
                            "profile":failure_profile,
                        }));
                    }
                    Ok(())
                }));
                let mut process_token = 0;
                core.add_ProcessFailed(&process_handler, &mut process_token)?;
                let mut hooks = Hooks { core, process_token, browser: None, navigation:None, frame_navigation:None, browser_logs:Vec::new() };
                for event in ["Runtime.exceptionThrown","Log.entryAdded","Runtime.consoleAPICalled"] {
                    let name=windows_core::HSTRING::from(event);
                    if let Ok(receiver)=hooks.core.GetDevToolsProtocolEventReceiver(windows_core::PCWSTR(name.as_ptr())) {
                        let event_label=label.clone();
                        let handler=DevToolsProtocolEventReceivedEventHandler::create(Box::new(move|_,args|{
                            if let Some(args)=args {
                                let mut raw=PWSTR::null();
                                if args.ParameterObjectAsJson(&mut raw).is_ok(){let raw=take_pwstr(raw);
                                    if raw.len()<=256*1024 {if let Ok(value)=serde_json::from_str::<serde_json::Value>(&raw){
                                        if super::super::cdp::accept(&event_label,event,&value){native_event(event_label.clone(),"webview.browser_error",super::super::cdp::summary(event,&value));}
                                    }}
                                    else if super::super::cdp::allow(&event_label,"oversized_browser_log"){native_event(event_label.clone(),"webview.browser_log_truncated",json!({"event":event,"bytes":raw.len()}));}
                                }
                            } Ok(())
                        }));
                        let mut token=0;if receiver.add_DevToolsProtocolEventReceived(&handler,&mut token).is_ok(){hooks.browser_logs.push((receiver,token));}
                    }
                }
                for method in ["Runtime.enable","Log.enable"] {
                    let event_label=label.clone();
                    let complete=CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move|result,_|{
                        native_event(event_label.clone(),"webview.browser_log_domain",json!({"method":method,"enabled":result.is_ok(),"hresult":result.as_ref().err().map(|error|error.code().0).unwrap_or(0)}));Ok(())
                    }));
                    let method_name=windows_core::HSTRING::from(method);
                    if let Err(error)=hooks.core.CallDevToolsProtocolMethod(windows_core::PCWSTR(method_name.as_ptr()),windows_core::w!("{}"),&complete){native_event(label.clone(),"webview.browser_log_enable_failed",json!({"method":method,"error":error.to_string()}));}
                }
                let navigation_label=label.clone();let mut token=0;
                let handler=NavigationCompletedEventHandler::create(Box::new(move|_,args|{navigation(&navigation_label,"main",args);Ok(())}));
                if hooks.core.add_NavigationCompleted(&handler,&mut token).is_ok() { hooks.navigation=Some(token); }
                let navigation_label=label.clone();let mut token=0;
                let handler=NavigationCompletedEventHandler::create(Box::new(move|_,args|{navigation(&navigation_label,"frame",args);Ok(())}));
                if hooks.core.add_FrameNavigationCompleted(&handler,&mut token).is_ok() { hooks.frame_navigation=Some(token); }
                if let Ok(environment5) = environment.cast::<ICoreWebView2Environment5>() {
                    let browser_label = label.clone();
                    let handler = BrowserProcessExitedEventHandler::create(Box::new(move |_, args| {
                        if let Some(args) = args {
                            let mut kind = COREWEBVIEW2_BROWSER_PROCESS_EXIT_KIND::default();
                            let mut pid = 0;
                            let kind_ok = args.BrowserProcessExitKind(&mut kind).is_ok();
                            let pid_ok = args.BrowserProcessId(&mut pid).is_ok();
                            native_event(browser_label.clone(), "webview.browser_exited", json!({"exit_kind":kind_ok.then_some(kind.0),"browser_pid":pid_ok.then_some(pid)}));
                        }
                        Ok(())
                    }));
                    let mut token = 0;
                    if environment5.add_BrowserProcessExited(&handler, &mut token).is_ok() { hooks.browser = Some((environment5, token)); }
                }
                native_event(label.clone(), "webview.capabilities", json!({"engine":"WebView2","process_failed":true,"browser_exited":hooks.browser.is_some(),"navigation_completed":hooks.navigation.is_some(),"frame_navigation_completed":hooks.frame_navigation.is_some(),"browser_log_receivers":hooks.browser_logs.len(),"browser_version":version,"failure_report_folder":failure_report_folder,"dumps_included_in_export":false}));
                HOOKS.with(|all| { all.borrow_mut().insert(label.clone(), hooks); });
                Ok(())
            })();
            if let Err(error) = result { native_event(label, "webview.hook_failed", json!({"error":error.to_string()})); }
        }) {
            native_event(dispatch_label, "webview.hook_failed", json!({"error":error.to_string()}));
        }
    }
    pub fn closed(label: &str) {
        super::super::cdp::closed(label);
        HOOKS.with(|hooks| {
            hooks.borrow_mut().remove(label);
        });
    }
}

#[cfg(windows)]
pub use windows_hooks::{closed, install};
#[cfg(not(windows))]
pub fn install(_webview: &tauri::Webview) {}
#[cfg(not(windows))]
pub fn closed(_label: &str) {}
