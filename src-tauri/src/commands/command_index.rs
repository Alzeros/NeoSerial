//! 知识库"手册指令索引"接入:拉取 + 本地缓存刷新;以及输入框发送历史的持久化命令。
//! 接口:GET {base}/api/v1/commands/documents(手册列表)、
//!       GET {base}/api/v1/commands?document_id=&page=&page_size=(某手册指令,分页)。
//! 鉴权头 X-API-Key;有 rpm 限流与日配额,所以按手册拉全量缓存,不逐字调用。
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::config::command_index::{merge_fetched, CommandIndexCache, ManualCommand, ManualDocument};
use crate::config::settings::CommandIndexSettings;
use crate::state::AppState;

/// 进程级"正在刷新"标志:设置页手动刷新与启动自动刷新不重叠。
/// 也是 CommandIndexCache::save_to 固定临时文件名的前提——同一时刻只有一个写入者。
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// 持有期间 REFRESHING=true,任何退出路径(含 ? 早退)都复位。
struct RefreshGuard;
impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESHING.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Serialize)]
pub struct RefreshResult {
    /// 有指令索引(cmd_status=done)的手册数
    pub doc_count: usize,
    pub cmd_count: usize,
    pub fetched_at: String,
    /// 拉失败、沿用旧缓存的手册标题
    pub failed: Vec<String>,
}

#[derive(Deserialize)]
struct CommandListOut {
    total: i64,
    items: Vec<ManualCommand>,
}

#[derive(Clone, Serialize)]
pub struct SendHistoryChanged {
    pub items: Vec<String>,
}

/// 接口错误码 → 中文原因(见接口文档"错误码"一节)。
pub fn map_http_error(status: u16) -> String {
    match status {
        401 => "API Key 无效".into(),
        403 => "应用已禁用".into(),
        404 => "接口不存在,请检查服务器地址".into(),
        429 => "触发限流,请稍后再试".into(),
        503 => "知识库对外 API 已关闭".into(),
        s => format!("HTTP {}", s),
    }
}

/// 去首尾空白与末尾 /,拼路径时统一用 `{base}/api/v1/...`。
pub fn normalize_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

/// reqwest::Error 的 Display 不带 source 链,"error sending request" 看不出是拒绝连接还是超时。
/// 这里把底层原因串上;超时单独说,用户最常撞的就是地址/端口填错和不在内网。
fn describe_reqwest_error(prefix: &str, e: &reqwest::Error) -> String {
    if e.is_timeout() {
        return format!("{}: 连接知识库超时(15s),检查地址、端口与内网连接", prefix);
    }
    let mut msg = format!("{}: {}", prefix, e);
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        msg.push_str(" ← ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    msg
}

/// 单页条数上限(接口允许 1–200)。
const PAGE_SIZE: usize = 200;
/// 翻页安全上限:一本手册最多 50 页 × 200 条,超出视为接口异常停止。
const MAX_PAGES: usize = 50;

/// GET 并解析 JSON。429 按 Retry-After(缺省 2s,上限 10s)等一次再试;其他非 2xx 映射为中文错误。
async fn get_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> Result<T, String> {
    let mut retried = false;
    loop {
        let resp = client
            .get(url)
            .header("X-API-Key", api_key)
            .send()
            .await
            .map_err(|e| describe_reqwest_error("无法连接服务器", &e))?;
        let status = resp.status();
        if status.as_u16() == 429 && !retried {
            let wait = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(2)
                .min(10);
            tokio::time::sleep(Duration::from_secs(wait)).await;
            retried = true;
            continue;
        }
        if !status.is_success() {
            return Err(map_http_error(status.as_u16()));
        }
        return resp
            .json::<T>()
            .await
            .map_err(|e| describe_reqwest_error("响应解析失败", &e));
    }
}

/// 拉一本手册的全部指令:page_size=200 翻页到 total 或空页。
async fn fetch_all_commands(
    client: &reqwest::Client,
    base: &str,
    api_key: &str,
    document_id: i64,
) -> Result<Vec<ManualCommand>, String> {
    let mut items = Vec::new();
    let mut complete = false;
    for page in 1..=MAX_PAGES {
        let url = format!(
            "{}/api/v1/commands?document_id={}&page={}&page_size={}",
            base, document_id, page, PAGE_SIZE
        );
        let out: CommandListOut = get_json(client, &url, api_key).await?;
        let got = out.items.len();
        items.extend(out.items);
        if got == 0 || items.len() as i64 >= out.total {
            complete = true;
            break;
        }
    }
    if !complete {
        log::warn!(
            "手册 {} 指令超过 {} 页 × {} 条上限,已截断",
            document_id, MAX_PAGES, PAGE_SIZE
        );
    }
    Ok(items)
}

/// 刷新主流程(不落盘、不广播):手册列表 → 逐本拉指令 → 与旧缓存合并。
/// 单本失败不中断,记进 failed 并沿用旧缓存;手册列表本身失败整体返错。
/// 对所有 cmd_status=done 的手册都拉,不按 disabled_doc_ids 跳过(那是前端显示层过滤)。
/// 调用方若要把返回的缓存 save() 落盘,必须先拿到 REFRESHING(见 command_index_refresh),
/// 否则破坏 save_to 的单写入者前提。
pub async fn refresh_impl(
    base_url: &str,
    api_key: &str,
) -> Result<(CommandIndexCache, Vec<String>), String> {
    let base = normalize_base_url(base_url);
    let key = api_key.trim().to_string();
    if base.is_empty() || key.is_empty() {
        return Err("请先填写知识库地址和 API Key".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let docs: Vec<ManualDocument> =
        get_json(&client, &format!("{}/api/v1/commands/documents", base), &key).await?;
    let old = tauri::async_runtime::spawn_blocking(CommandIndexCache::load)
        .await
        .map_err(|e| format!("读取旧缓存任务执行异常: {}", e))?;

    let mut fetched: HashMap<i64, Vec<ManualCommand>> = HashMap::new();
    let mut failed_ids = Vec::new();
    let mut failed_titles = Vec::new();
    for d in docs.iter().filter(|d| d.cmd_status == "done") {
        match fetch_all_commands(&client, &base, &key, d.id).await {
            Ok(items) => {
                fetched.insert(d.id, items);
            }
            Err(e) => {
                log::warn!("拉取手册 {}({}) 指令失败: {}", d.title, d.id, e);
                failed_ids.push(d.id);
                failed_titles.push(d.title.clone());
            }
        }
    }
    let cache = merge_fetched(
        docs,
        fetched,
        &failed_ids,
        &old,
        &base,
        chrono::Local::now().to_rfc3339(),
    );
    Ok((cache, failed_titles))
}

/// 设置页"刷新指令库":拉取 → 写缓存 → 广播 command-index-changed → 返回统计。
/// 用传入的地址/Key(设置页编辑框里的当前值,不要求先保存)。
#[tauri::command]
pub async fn command_index_refresh(
    app_handle: tauri::AppHandle,
    base_url: String,
    api_key: String,
) -> Result<RefreshResult, String> {
    if REFRESHING.swap(true, Ordering::SeqCst) {
        return Err("正在刷新,请稍候".into());
    }
    let _guard = RefreshGuard;
    let (cache, failed) = refresh_impl(&base_url, &api_key).await?;
    // 统计先算出来,cache 整份移进落盘闭包,不为了取三个数多复制一份指令表
    let doc_count = cache.documents.iter().filter(|d| d.cmd_status == "done").count();
    let cmd_count = cache.commands.len();
    let fetched_at = cache.fetched_at.clone().unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || cache.save())
        .await
        .map_err(|e| format!("写缓存任务执行异常: {}", e))?
        .map_err(|e| format!("写入缓存失败: {}", e))?;
    let _ = app_handle.emit("command-index-changed", ());
    Ok(RefreshResult {
        doc_count,
        cmd_count,
        fetched_at,
        failed,
    })
}

/// 前端启动 / 收到 command-index-changed 时调:读本地缓存。从未刷新过返回空缓存。
#[tauri::command]
pub async fn command_index_load() -> Result<CommandIndexCache, String> {
    tauri::async_runtime::spawn_blocking(CommandIndexCache::load)
        .await
        .map_err(|e| format!("读取缓存任务执行异常: {}", e))
}

/// setup 时调一次:配了地址和 Key 且 auto_refresh 开 → 后台刷新,失败只记日志。
/// 进程级只跑一次,不按窗口跑(多窗口各跑一遍会撞 REFRESHING 也浪费配额)。
pub fn spawn_auto_refresh(handle: &tauri::AppHandle, cfg: &CommandIndexSettings) {
    if !cfg.auto_refresh || cfg.base_url.trim().is_empty() || cfg.api_key.trim().is_empty() {
        return;
    }
    let handle = handle.clone();
    let (base_url, api_key) = (cfg.base_url.clone(), cfg.api_key.clone());
    tauri::async_runtime::spawn(async move {
        match command_index_refresh(handle, base_url, api_key).await {
            Ok(r) => log::info!(
                "启动刷新指令库完成: {} 本手册 {} 条指令, 失败 {} 本",
                r.doc_count,
                r.cmd_count,
                r.failed.len()
            ),
            Err(e) => log::warn!("启动刷新指令库失败: {}", e),
        }
    });
}

// ============ 发送历史 ============

/// 输入框手动发送成功后调:记一条(去重挪前、上限 500)、落盘、广播给所有窗口。返回最新全量列表。
/// 已在最前的重复发送不落盘不广播。落盘失败只返错,内存里已记下的那条保留(历史以内存为准)。
#[tauri::command]
pub async fn send_history_push(
    app_handle: tauri::AppHandle,
    text: String,
) -> Result<Vec<String>, String> {
    let handle = app_handle.clone();
    let (changed, items) = tauri::async_runtime::spawn_blocking(
        move || -> Result<(bool, Vec<String>), String> {
            let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
            let mut h = state.send_history.lock().map_err(|e| e.to_string())?;
            let changed = h.push(&text);
            if changed {
                h.save().map_err(|e| format!("写入发送历史失败: {}", e))?;
            }
            Ok((changed, h.items().to_vec()))
        },
    )
    .await
    .map_err(|e| format!("记录发送历史任务执行异常: {}", e))??;
    if changed {
        let _ = app_handle.emit(
            "send-history-changed",
            SendHistoryChanged { items: items.clone() },
        );
    }
    Ok(items)
}

#[tauri::command]
pub fn send_history_load(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let h = state.send_history.lock().map_err(|e| e.to_string())?;
    Ok(h.items().to_vec())
}

/// 设置页"清空":清空、落盘、广播空列表。
#[tauri::command]
pub async fn send_history_clear(app_handle: tauri::AppHandle) -> Result<(), String> {
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        let mut h = state.send_history.lock().map_err(|e| e.to_string())?;
        h.clear();
        h.save().map_err(|e| format!("写入发送历史失败: {}", e))
    })
    .await
    .map_err(|e| format!("清空发送历史任务执行异常: {}", e))??;
    let _ = app_handle.emit("send-history-changed", SendHistoryChanged { items: Vec::new() });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_http_error_known_codes() {
        assert_eq!(map_http_error(401), "API Key 无效");
        assert_eq!(map_http_error(403), "应用已禁用");
        assert_eq!(map_http_error(429), "触发限流,请稍后再试");
        assert_eq!(map_http_error(503), "知识库对外 API 已关闭");
        assert_eq!(map_http_error(500), "HTTP 500");
    }

    #[test]
    fn test_normalize_base_url_strips_trailing_slash_and_space() {
        assert_eq!(normalize_base_url(" http://10.12.16.11:8200/ "), "http://10.12.16.11:8200");
        assert_eq!(normalize_base_url("http://h:8200//"), "http://h:8200");
        assert_eq!(normalize_base_url("   "), "");
    }

    /// 接口 { total, items[] } 形态(items 带 created_at、page_no 有值)能解析。
    #[test]
    fn test_command_list_out_parses_api_shape() {
        let json = r#"{"total":1,"items":[{"id":1,"document_id":3,"command":"AT+MQTTCFG",
            "name":"配置","syntax":"AT+MQTTCFG=\"<param>\"","parameters":[],"example":"",
            "page_no":7,"summary":"s","created_at":"x"}]}"#;
        let out: CommandListOut = serde_json::from_str(json).unwrap();
        assert_eq!(out.total, 1);
        assert_eq!(out.items[0].page_no, Some(7));
        assert_eq!(out.items[0].command, "AT+MQTTCFG");
    }
}
