//! Browser error metadata received by the host even before page diagnostics can start.
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

fn url(value: &str) -> String {
    if value
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
    {
        return "data:[redacted]".to_string();
    }
    let value = value.split(['?', '#']).next().unwrap_or("");
    if let Some((scheme, rest)) = value.split_once("://") {
        let (authority, path) = rest
            .split_once('/')
            .map(|(host, path)| (host, format!("/{}", path)))
            .unwrap_or((rest, String::new()));
        return format!(
            "{}://{}{}",
            scheme,
            authority.rsplit('@').next().unwrap_or(""),
            path
        )
        .chars()
        .take(1500)
        .collect();
    }
    value.chars().take(1500).collect()
}
fn text(value: &str) -> String {
    static AUTH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)\b((?:Bearer|Basic)\s+)[^\s"',;]+"#).unwrap());
    static SECRET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)(["']?(?:api[_-]?key|token|password|secret|authorization)["']?\s*[:=]\s*)(?:"[^"\n]*"|'[^'\n]*'|[^\s,;}]+)"#).unwrap()
    });
    static URL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?:https?|wss?|tauri|file)://[^\s<>"')]+"#).unwrap());
    let short: String = value.chars().take(6000).collect();
    let auth = AUTH.replace_all(&short, "${1}[redacted]");
    let secret = SECRET.replace_all(&auth, "${1}[redacted]");
    URL.replace_all(&secret, |captures: &regex::Captures<'_>| url(&captures[0]))
        .into_owned()
}

pub fn summary(method: &str, data: &Value) -> Value {
    if method == "Runtime.exceptionThrown" {
        let detail = &data["exceptionDetails"];
        let frames=detail["stackTrace"]["callFrames"].as_array().map(|frames|frames.iter().take(30).map(|frame|json!({
            "function":text(frame["functionName"].as_str().unwrap_or("")),"url":url(frame["url"].as_str().unwrap_or("")),"line":frame["lineNumber"],"column":frame["columnNumber"],
        })).collect::<Vec<_>>()).unwrap_or_default();
        json!({"event":method,"line_base":0,"source_timestamp":data["timestamp"],"message":text(detail["exception"]["description"].as_str().or_else(||detail["text"].as_str()).unwrap_or("")),
            "class":detail["exception"]["className"],"url":url(detail["url"].as_str().unwrap_or("")),"line":detail["lineNumber"],"column":detail["columnNumber"],"frames":frames})
    } else if method == "Runtime.consoleAPICalled" {
        let args=data["args"].as_array().map(|args|args.iter().take(3).map(|arg|{
            if arg["type"]=="string" {json!({"type":"string","message":text(arg["value"].as_str().unwrap_or(""))})}
            else if arg["subtype"]=="error" {json!({"type":"error","message":text(arg["description"].as_str().unwrap_or(""))})}
            else {json!({"type":arg["type"],"class":arg["className"]})}
        }).collect::<Vec<_>>()).unwrap_or_default();
        json!({"event":method,"source_timestamp":data["timestamp"],"level":data["type"],"context":data["executionContextId"],"args":args})
    } else {
        let entry = &data["entry"];
        json!({"event":method,"line_base":0,"source_timestamp":entry["timestamp"],"source":entry["source"],"level":entry["level"],"message":text(entry["text"].as_str().unwrap_or("")),"url":url(entry["url"].as_str().unwrap_or("")),"line":entry["lineNumber"]})
    }
}

struct Rate {
    start: Instant,
    count: u32,
    omitted: u64,
}
static RATES: LazyLock<Mutex<HashMap<(String, String), Rate>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub fn allow(window: &str, method: &str) -> bool {
    let Ok(mut rates) = RATES.lock() else {
        return false;
    };
    let now = Instant::now();
    let rate = rates
        .entry((window.to_string(), method.to_string()))
        .or_insert(Rate {
            start: now,
            count: 0,
            omitted: 0,
        });
    if now.duration_since(rate.start) >= Duration::from_secs(10) {
        rate.start = now;
        rate.count = 0;
    }
    rate.count += 1;
    if rate.count > 200 {
        rate.omitted += 1;
        false
    } else {
        true
    }
}
pub fn accept(window: &str, method: &str, data: &Value) -> bool {
    // Ordinary console traffic must not spend the budget reserved for errors.
    if method == "Runtime.consoleAPICalled"
        && !matches!(data["type"].as_str(), Some("error" | "warning" | "assert"))
    {
        return false;
    }
    if method == "Log.entryAdded"
        && !matches!(data["entry"]["level"].as_str(), Some("error" | "warning"))
    {
        return false;
    }
    allow(window, method)
}
pub fn flush() {
    if let Ok(mut rates) = RATES.lock() {
        for ((window, method), rate) in rates.iter_mut() {
            if rate.omitted > 0 {
                super::frontend::native_event(
                    window.clone(),
                    "webview.browser_log_throttled",
                    json!({"event":method,"count":std::mem::take(&mut rate.omitted)}),
                );
            }
        }
    }
}
pub fn closed(window: &str) {
    if let Ok(mut rates) = RATES.lock() {
        rates.retain(|(label, _), _| label != window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preboot_exceptions_keep_source_locations_but_not_remote_objects_or_credentials() {
        let raw = json!({"exceptionDetails":{"text":"Uncaught","url":"https://user:PRIVATE_PASS@example.com/app.js?token=PRIVATE_QUERY","lineNumber":41,"columnNumber":8,"exception":{"className":"TypeError","description":"TypeError: boot failed; api_key=PRIVATE_KEY","objectId":"PRIVATE_OBJECT"},"stackTrace":{"callFrames":[{"functionName":"boot","url":"tauri://localhost/assets/index.js?token=PRIVATE_FRAME","lineNumber":41,"columnNumber":8}]}}});
        let value = summary("Runtime.exceptionThrown", &raw);
        let encoded = value.to_string();
        assert!(!encoded.contains("PRIVATE_"));
        assert_eq!(value["line"], 41);
        assert_eq!(value["frames"][0]["function"], "boot");
    }
    #[test]
    fn browser_resource_errors_are_available_without_page_javascript() {
        let value = summary(
            "Log.entryAdded",
            &json!({"entry":{"source":"network","level":"error","text":"Failed to load resource","url":"tauri://localhost/assets/missing.js?token=PRIVATE"}}),
        );
        assert_eq!(value["source"], "network");
        assert!(!value.to_string().contains("PRIVATE"));
    }
    #[test]
    fn caught_boot_console_errors_do_not_expand_remote_objects() {
        let value = summary(
            "Runtime.consoleAPICalled",
            &json!({"type":"error","executionContextId":1,"args":[{"type":"string","value":"api_key=PRIVATE_SECRET"},{"type":"object","objectId":"PRIVATE_OBJECT","preview":{"password":"PRIVATE_PASSWORD"}}]}),
        );
        assert!(!value.to_string().contains("PRIVATE_"));
        assert_eq!(value["level"], "error");
    }
    #[test]
    fn browser_noise_cannot_consume_another_error_channels_budget() {
        let window = "cdp-test-rate";
        closed(window);
        for _ in 0..200 {
            assert!(allow(window, "Runtime.consoleAPICalled"));
        }
        assert!(!allow(window, "Runtime.consoleAPICalled"));
        assert!(allow(window, "Runtime.exceptionThrown"));
        closed(window);
        assert!(allow(window, "Runtime.consoleAPICalled"));
        closed(window);
    }

    #[test]
    fn informational_console_messages_do_not_suppress_later_warnings_and_errors() {
        let window = "cdp-test-console-filter";
        for _ in 0..1000 {
            assert!(!accept(
                window,
                "Runtime.consoleAPICalled",
                &json!({"type":"log"})
            ));
        }
        assert!(accept(
            window,
            "Runtime.consoleAPICalled",
            &json!({"type":"error"})
        ));
        assert!(accept(
            window,
            "Runtime.consoleAPICalled",
            &json!({"type":"warning"})
        ));
        closed(window);
    }

    #[test]
    fn inline_source_urls_never_store_embedded_code_and_external_urls_are_bounded() {
        let value = summary(
            "Runtime.exceptionThrown",
            &json!({"exceptionDetails":{"url":"data:text/javascript,PRIVATE_SOURCE","stackTrace":{"callFrames":[{"url":"data:text/javascript,PRIVATE_FRAME"}]}}}),
        );
        assert!(!value.to_string().contains("PRIVATE_"));
        assert_eq!(value["url"], "data:[redacted]");
        assert_eq!(
            url(&format!(
                "https://user:PRIVATE_PASSWORD@example.com/{}?token=PRIVATE_TOKEN",
                "a".repeat(3000)
            ))
            .chars()
            .count(),
            1500
        );
    }
}
