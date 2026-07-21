mod db;
mod diagnostics;
mod digest;
mod fetcher;
mod hn;
mod pi_bridge;
mod tool_cli;

use db::{Article, Feed};
use diagnostics::{DiagnosticInfo, DiagnosticLogger};
use pi_bridge::PiBridge;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

pub(crate) struct AppState {
    pub(crate) db: Mutex<Connection>,
    pub(crate) client: reqwest::Client,
    pub(crate) diagnostics: DiagnosticLogger,
    pi: PiBridge,
}

#[derive(Serialize)]
struct RefreshResult {
    new_articles: usize,
    failed: Vec<String>,
}

pub(crate) fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
    state
        .db
        .lock()
        .map_err(|_| "DBロックに失敗しました".to_string())
}

#[tauri::command]
fn list_feeds(state: State<AppState>) -> Result<Vec<Feed>, String> {
    let conn = lock_db(&state)?;
    db::list_feeds(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_articles(
    state: State<AppState>,
    feed_id: Option<i64>,
    category: Option<String>,
    unread_only: bool,
    starred_only: bool,
) -> Result<Vec<Article>, String> {
    let conn = lock_db(&state)?;
    db::list_articles(
        &conn,
        feed_id,
        category.as_deref(),
        unread_only,
        starred_only,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_article(state: State<AppState>, article_id: i64) -> Result<Article, String> {
    let conn = lock_db(&state)?;
    db::get_article(&conn, article_id).map_err(|e| e.to_string())
}

const DEFAULT_SUMMARY_MODEL: &str = "openai-codex/gpt-5.6-luna";

#[tauri::command]
async fn summarize_article(
    app: AppHandle,
    article_id: i64,
    force: bool,
) -> Result<Article, String> {
    let (article, model, client) = {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        let article = db::get_article(&conn, article_id).map_err(|e| e.to_string())?;
        if !force && article.ai_summary.is_some() {
            return Ok(article);
        }
        let model = db::get_setting(&conn, "summary_model")
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SUMMARY_MODEL.to_string());
        (article, model, state.client.clone())
    };

    let text = if let Some(text) = article.full_text.as_deref() {
        text.to_string()
    } else {
        let fetched = match article.url.as_deref() {
            Some(url) => fetcher::fetch_page_text(&client, url, 24_000).await.ok(),
            None => None,
        };
        let text = fetched
            .or_else(|| {
                article
                    .content_html
                    .as_deref()
                    .map(|html| fetcher::strip_html(html, 24_000))
            })
            .or_else(|| article.summary.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or("要約できる本文がありません")?;
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        db::store_full_text(&conn, article_id, &text).map_err(|e| e.to_string())?;
        text
    };

    let summary = digest::summarize_article_text(&article.title, &text, &model).await?;
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    db::store_ai_summary(&conn, article_id, &summary, &model).map_err(|e| e.to_string())?;
    db::get_article(&conn, article_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn fuzzy_search(state: State<AppState>, query: String) -> Result<Vec<Article>, String> {
    let conn = lock_db(&state)?;
    db::search_articles(&conn, &query, 200).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_setting(state: State<AppState>, key: String) -> Result<Option<String>, String> {
    let conn = lock_db(&state)?;
    db::get_setting(&conn, &key).map_err(|e| e.to_string())
}

#[tauri::command]
fn seed_e2e_data(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    if !cfg!(debug_assertions) || std::env::var("MYFOCUS_E2E").as_deref() != Ok("1") {
        return Err("E2Eビルドでのみ利用できます".to_string());
    }
    let conn = lock_db(&state)?;
    conn.execute_batch(
        "DELETE FROM articles;
         DELETE FROM feeds;
         INSERT INTO feeds (id, url, title, category) VALUES
           (101, 'https://e2e.example/alpha.xml', 'Alpha Feed', NULL),
           (102, 'https://e2e.example/beta.xml', 'Beta Feed', 'Tech');
         INSERT INTO articles
           (id, feed_id, guid, title, summary, published_at, read, starred) VALUES
           (1001, 101, 'alpha-1', 'Alpha searchable story', 'Rust desktop testing guide', 300, 0, 0),
           (1002, 101, 'alpha-2', 'Second Alpha story', 'Keyboard navigation article', 200, 0, 0),
           (1003, 102, 'beta-1', 'Beta technology story', 'Tauri and WebDriver integration', 100, 0, 0);",
    )
    .map_err(|e| e.to_string())?;
    let _ = app.emit("feeds-updated", ());
    Ok(())
}

#[tauri::command]
fn set_setting(
    app: AppHandle,
    state: State<AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    {
        let conn = lock_db(&state)?;
        db::set_setting(&conn, &key, &value).map_err(|e| e.to_string())?;
    }
    if key == "diagnostic_logging_enabled" {
        state.diagnostics.set_enabled(value == "true");
    }
    state.diagnostics.log(
        "info",
        "setting_updated",
        Some(serde_json::json!({ "key": key })),
    );
    let _ = app.emit("settings-updated", &key);
    Ok(())
}

#[tauri::command]
fn diagnostic_log(
    state: State<AppState>,
    level: String,
    event: String,
    details: Option<serde_json::Value>,
) -> Result<(), String> {
    if event.trim().is_empty() {
        return Err("ログイベント名が空です".to_string());
    }
    state.diagnostics.log(&level, &event, details);
    Ok(())
}

#[tauri::command]
fn get_diagnostic_info(state: State<AppState>) -> DiagnosticInfo {
    state.diagnostics.info()
}

#[tauri::command]
fn clear_diagnostic_logs(state: State<AppState>) -> Result<(), String> {
    state.diagnostics.clear().map_err(|e| e.to_string())
}

#[tauri::command]
fn open_diagnostic_folder(state: State<AppState>) -> Result<(), String> {
    let directory = state.diagnostics.directory();
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer.exe")
        .arg(directory)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(directory)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    return Err("この機能はmacOSとWindowsのみ対応です".to_string());
    Ok(())
}

#[tauri::command]
fn list_ai_feedback(state: State<AppState>) -> Result<HashMap<i64, i8>, String> {
    let conn = lock_db(&state)?;
    db::list_ai_feedback(&conn)
        .map(|items| items.into_iter().collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_ai_feedback(state: State<AppState>, article_id: i64, value: i8) -> Result<(), String> {
    if ![-1, 0, 1].contains(&value) {
        return Err("フィードバック値が不正です".to_string());
    }
    let conn = lock_db(&state)?;
    db::set_ai_feedback(&conn, article_id, value).map_err(|e| e.to_string())?;
    // Interest changes invalidate the daily recommendation response.
    db::set_setting(&conn, "ai_recommendation_cache", "").map_err(|e| e.to_string())
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("設定ウィンドウが見つかりません")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn close_settings(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("設定ウィンドウが見つかりません")?;
    window.hide().map_err(|e| e.to_string())
}

#[tauri::command]
fn is_settings_visible(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("設定ウィンドウが見つかりません")?;
    window.is_visible().map_err(|e| e.to_string())
}

#[tauri::command]
fn request_settings_native_close(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("設定ウィンドウが見つかりません")?;
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn mark_read(state: State<AppState>, article_id: i64, read: bool) -> Result<(), String> {
    let conn = lock_db(&state)?;
    db::set_read(&conn, article_id, read).map_err(|e| e.to_string())
}

#[tauri::command]
fn mark_starred(state: State<AppState>, article_id: i64, starred: bool) -> Result<(), String> {
    let conn = lock_db(&state)?;
    db::set_starred(&conn, article_id, starred).map_err(|e| e.to_string())
}

#[tauri::command]
fn mark_all_read(
    state: State<AppState>,
    feed_id: Option<i64>,
    category: Option<String>,
) -> Result<(), String> {
    let conn = lock_db(&state)?;
    db::mark_all_read(&conn, feed_id, category.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_feed(state: State<AppState>, feed_id: i64) -> Result<(), String> {
    let conn = lock_db(&state)?;
    db::remove_feed(&conn, feed_id).map_err(|e| e.to_string())
}

// --- Hacker News（RSSとは独立した情報ソースモジュール） ---

#[tauri::command]
fn hn_list(state: State<AppState>) -> Result<Vec<hn::HnItem>, String> {
    let conn = lock_db(&state)?;
    hn::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
async fn hn_refresh(app: AppHandle) -> Result<usize, String> {
    hn::refresh(&app).await
}

#[tauri::command]
async fn hn_summarize_comments(app: AppHandle, item_id: i64) -> Result<String, String> {
    hn::summarize_comments(&app, item_id).await
}

#[tauri::command]
async fn list_pi_models() -> Result<Vec<String>, String> {
    digest::list_models().await
}

/// Find a feed URL inside an HTML page (`<link rel="alternate" type="application/rss+xml">`).
fn discover_feed_url(html: &str, base: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut pos = 0;
    while let Some(start) = lower[pos..].find("<link") {
        let start = pos + start;
        let end = lower[start..].find('>').map(|e| start + e)?;
        let tag = &html[start..=end];
        let tag_lower = &lower[start..=end];
        if tag_lower.contains("alternate")
            && (tag_lower.contains("application/rss+xml")
                || tag_lower.contains("application/atom+xml"))
        {
            if let Some(href) = extract_attr(tag, "href") {
                if href.starts_with("http") {
                    return Some(href);
                }
                // resolve relative URL against the page origin
                if let Ok(base_url) = reqwest::Url::parse(base) {
                    if let Ok(joined) = base_url.join(&href) {
                        return Some(joined.to_string());
                    }
                }
            }
        }
        pos = end + 1;
    }
    None
}

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let idx = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[idx..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        rest[1..].split(quote).next().map(|s| s.to_string())
    } else {
        rest.split([' ', '>', '/']).next().map(|s| s.to_string())
    }
}

#[tauri::command]
async fn add_feed(app: AppHandle, url: String) -> Result<Feed, String> {
    let state = app.state::<AppState>();
    let client = state.client.clone();

    let (feed_url, fetched) = match fetcher::fetch_feed(&client, &url).await {
        Ok(f) => (url.clone(), f),
        Err(first_err) => {
            // maybe an HTML page — try feed autodiscovery
            let res = client
                .get(&url)
                .send()
                .await
                .map_err(|_| first_err.clone())?;
            let html = res.text().await.map_err(|_| first_err.clone())?;
            let discovered = discover_feed_url(&html, &url).ok_or(first_err)?;
            let fetched = fetcher::fetch_feed(&client, &discovered).await?;
            (discovered, fetched)
        }
    };

    let feed = {
        let state = app.state::<AppState>();
        let mut conn = lock_db(&state)?;
        let feed_id = db::upsert_feed(
            &conn,
            &feed_url,
            &fetched.title,
            fetched.site_url.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        db::insert_articles(&mut conn, feed_id, &fetched.articles).map_err(|e| e.to_string())?;
        db::set_setting(&conn, "ai_recommendation_cache", "").map_err(|e| e.to_string())?;
        db::list_feeds(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|f| f.id == feed_id)
            .ok_or("追加したフィードが見つかりません")?
    };
    let _ = app.emit("feeds-updated", ());
    Ok(feed)
}

async fn refresh_all_inner(app: &AppHandle) -> Result<RefreshResult, String> {
    let state = app.state::<AppState>();
    let client = state.client.clone();
    let feeds = {
        let conn = lock_db(&state)?;
        db::feed_urls(&conn).map_err(|e| e.to_string())?
    };

    let mut new_articles = 0;
    let mut failed = Vec::new();
    // fetch in windows of 10 concurrent requests
    for chunk in feeds.chunks(10) {
        let mut set = tokio::task::JoinSet::new();
        for (feed_id, url) in chunk {
            let client = client.clone();
            let feed_id = *feed_id;
            let url = url.clone();
            set.spawn(async move {
                let result = fetcher::fetch_feed(&client, &url).await;
                (feed_id, url, result)
            });
        }
        while let Some(joined) = set.join_next().await {
            let Ok((feed_id, url, result)) = joined else {
                continue;
            };
            match result {
                Ok(fetched) => {
                    let mut conn = lock_db(&state)?;
                    let _ =
                        db::upsert_feed(&conn, &url, &fetched.title, fetched.site_url.as_deref());
                    match db::insert_articles(&mut conn, feed_id, &fetched.articles) {
                        Ok(n) => {
                            new_articles += n;
                            let _ = db::set_feed_error(&conn, feed_id, None);
                        }
                        Err(e) => {
                            let _ = db::set_feed_error(&conn, feed_id, Some(&e.to_string()));
                            failed.push(format!("{url}: {e}"));
                        }
                    }
                }
                Err(e) => {
                    let conn = lock_db(&state)?;
                    let _ = db::set_feed_error(&conn, feed_id, Some(&e));
                    failed.push(format!("{url}: {e}"));
                }
            }
        }
        // let the UI update progressively during large refreshes
        let _ = app.emit("feeds-updated", ());
    }
    let _ = app.emit("feeds-updated", ());
    if new_articles > 0 {
        let conn = lock_db(&state)?;
        // A briefing generated before this refresh no longer covers the inbox.
        db::set_setting(&conn, "ai_recommendation_cache", "").map_err(|e| e.to_string())?;
        let _ = app.emit("recommendation-cache-invalidated", new_articles);
    }
    state.diagnostics.log(
        if failed.is_empty() { "info" } else { "warn" },
        "feeds_refreshed",
        Some(serde_json::json!({
            "feedCount": feeds.len(),
            "newArticles": new_articles,
            "failedCount": failed.len(),
        })),
    );
    Ok(RefreshResult {
        new_articles,
        failed,
    })
}

#[tauri::command]
async fn refresh_all(app: AppHandle) -> Result<RefreshResult, String> {
    refresh_all_inner(&app).await
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

struct OpmlFeed {
    url: String,
    title: String,
    category: Option<String>,
}

/// Parse OPML text into feeds, resolving each feed's innermost enclosing
/// folder outline as its category.
fn parse_opml(content: &str) -> Vec<OpmlFeed> {
    let lower = content.to_lowercase();
    let mut feeds = Vec::new();
    // stack of open <outline> elements: folder outlines carry their name,
    // feed outlines push an empty entry to keep the nesting balanced
    let mut stack: Vec<String> = Vec::new();
    let mut pos = 0;
    loop {
        let open = lower[pos..].find("<outline").map(|i| pos + i);
        let close = lower[pos..].find("</outline").map(|i| pos + i);
        match (open, close) {
            (Some(o), c) if c.is_none_or(|c| o < c) => {
                let Some(end) = lower[o..].find('>').map(|e| o + e) else {
                    break;
                };
                let tag = &content[o..=end];
                let self_closing = tag[..tag.len() - 1].trim_end().ends_with('/');
                if let Some(xml_url) = extract_attr(tag, "xmlurl") {
                    let title = extract_attr(tag, "title")
                        .or_else(|| extract_attr(tag, "text"))
                        .unwrap_or_else(|| xml_url.clone());
                    feeds.push(OpmlFeed {
                        url: xml_unescape(&xml_url),
                        title: xml_unescape(&title),
                        category: stack.iter().rev().find(|s| !s.is_empty()).cloned(),
                    });
                    if !self_closing {
                        stack.push(String::new());
                    }
                } else if !self_closing {
                    let name = extract_attr(tag, "title")
                        .or_else(|| extract_attr(tag, "text"))
                        .map(|s| xml_unescape(&s))
                        .unwrap_or_default();
                    stack.push(name);
                }
                pos = end + 1;
            }
            (_, Some(c)) => {
                stack.pop();
                pos = c + "</outline".len();
            }
            // (Some, None) は上のガードで必ず処理される
            _ => break,
        }
    }
    feeds
}

/// Import feeds from OPML text, preserving folder outlines as categories.
/// Returns the number of newly registered feeds; fetching happens in the
/// background afterwards.
#[tauri::command]
async fn import_opml(app: AppHandle, content: String) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let mut added = 0;
    {
        let conn = lock_db(&state)?;
        for feed in parse_opml(&content) {
            if db::insert_feed_stub(&conn, &feed.url, &feed.title, feed.category.as_deref())
                .map_err(|e| e.to_string())?
            {
                added += 1;
            }
        }
    }
    let _ = app.emit("feeds-updated", ());
    // fetch the imported feeds in the background
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = refresh_all_inner(&handle).await;
    });
    Ok(added)
}

/// Open a URL in the default browser while keeping the reader frontmost.
/// Some browsers activate themselves even when asked not to, so supported
/// desktop platforms grab focus back right afterwards.
#[tauri::command]
async fn open_background(app: AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("http(s)のURLのみ開けます".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        let win = app.get_webview_window("main");
        // 呼び出し時点でこちらが前面だった場合のみフォーカス奪還の対象にする
        let was_focused = win
            .as_ref()
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);

        std::process::Command::new("open")
            .args(["-g", &url])
            .spawn()
            .map_err(|e| format!("ブラウザで開けません: {e}"))?;

        // Chromeはウィンドウ状態によって -g を無視して前面化することがある。
        // フォーカスを失っていたら取り戻す（既に前面なら何もしない）
        if was_focused {
            for delay_ms in [200u64, 450, 900] {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                if let Some(win) = app.get_webview_window("main") {
                    if !win.is_focused().unwrap_or(false) {
                        let _ = win.set_focus();
                    }
                }
            }
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let win = app.get_webview_window("main");
        let was_focused = win
            .as_ref()
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);

        // Avoid cmd.exe so URL metacharacters such as '&' are not interpreted
        // by a shell. rundll32 delegates the URL to Windows' default handler.
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", &url])
            .spawn()
            .map_err(|e| format!("ブラウザで開けません: {e}"))?;

        if was_focused {
            for delay_ms in [200u64, 450, 900] {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                if let Some(win) = app.get_webview_window("main") {
                    if !win.is_focused().unwrap_or(false) {
                        let _ = win.set_focus();
                    }
                }
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Err("この機能はmacOSとWindowsのみ対応です".to_string())
    }
}

#[tauri::command]
async fn ai_prompt(app: AppHandle, message: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.pi.prompt(&app, &message).await
}

#[tauri::command]
async fn ai_abort(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.pi.abort(&app).await
}

#[tauri::command]
async fn ai_new_session(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.pi.new_session(&app).await
}

const POLL_INTERVAL: Duration = Duration::from_secs(15 * 60);
const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Delete old read articles according to the retention setting (0 disables).
fn run_cleanup(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(conn) = lock_db(&state) else { return };
    let days = db::get_setting(&conn, "retention_days")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    if days > 0 {
        match db::cleanup_old_articles(&conn, days) {
            Ok(deleted) if deleted > 0 => state.diagnostics.log(
                "info",
                "article_cleanup_completed",
                Some(serde_json::json!({ "deleted": deleted, "retentionDays": days })),
            ),
            Ok(_) => {}
            Err(error) => state.diagnostics.log(
                "error",
                "article_cleanup_failed",
                Some(serde_json::json!({ "message": error.to_string() })),
            ),
        }
    }
}

pub fn run_tool_cli(args: &[String]) -> Result<(), String> {
    tool_cli::run(args)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    #[cfg(debug_assertions)]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .setup(|app| {
            let data_dir = match std::env::var_os("MYFOCUS_DATA_DIR") {
                Some(path) => std::path::PathBuf::from(path),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("myfocus.db");
            let conn = db::open(&db_path)?;
            digest::init(&conn)?;
            hn::init(&conn)?;
            let logging_enabled =
                db::get_setting(&conn, "diagnostic_logging_enabled")?.as_deref() == Some("true");
            let diagnostics = DiagnosticLogger::new(data_dir.join("logs"), logging_enabled);
            diagnostics.log(
                "info",
                "app_started",
                Some(serde_json::json!({ "e2e": std::env::var_os("MYFOCUS_E2E").is_some() })),
            );

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?;

            app.manage(AppState {
                db: Mutex::new(conn),
                client,
                diagnostics,
                pi: PiBridge::new(db_path, std::env::current_exe()?),
            });

            // Keep the statically configured settings webview alive. Both the
            // native title-bar close button and the in-page × only hide it.
            if let Some(settings) = app.get_webview_window("settings") {
                let window = settings.clone();
                settings.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                });
            }

            // background poller: refresh on launch, then every 15 minutes.
            // E2E runs use an isolated DB and avoid external network traffic.
            if std::env::var_os("MYFOCUS_E2E").is_none() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        run_cleanup(&handle);
                        let _ = hn::refresh(&handle).await;
                        let _ = refresh_all_inner(&handle).await;
                        tokio::time::sleep(POLL_INTERVAL).await;
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_feeds,
            list_articles,
            get_article,
            summarize_article,
            fuzzy_search,
            get_setting,
            seed_e2e_data,
            set_setting,
            diagnostic_log,
            get_diagnostic_info,
            clear_diagnostic_logs,
            open_diagnostic_folder,
            list_ai_feedback,
            set_ai_feedback,
            open_settings,
            close_settings,
            is_settings_visible,
            request_settings_native_close,
            mark_read,
            mark_starred,
            mark_all_read,
            remove_feed,
            hn_list,
            hn_refresh,
            hn_summarize_comments,
            list_pi_models,
            add_feed,
            refresh_all,
            import_opml,
            open_background,
            ai_prompt,
            ai_abort,
            ai_new_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::db;
    use super::parse_opml;

    fn test_db() -> rusqlite::Connection {
        let dir = std::env::temp_dir().join(format!("myfocus-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("t{:?}.db", std::thread::current().id()));
        let _ = std::fs::remove_file(&path);
        db::open(&path).unwrap()
    }

    fn seed_article(conn: &mut rusqlite::Connection, feed_id: i64, title: &str, age_days: i64) {
        let ts = chrono::Utc::now().timestamp() - age_days * 86400;
        db::insert_articles(
            conn,
            feed_id,
            &[db::NewArticle {
                guid: title.to_string(),
                title: title.to_string(),
                url: None,
                author: None,
                summary: Some(format!("{title} の要約テキスト")),
                content_html: None,
                published_at: Some(ts),
            }],
        )
        .unwrap();
    }

    #[test]
    fn fts_search_matches_japanese_and_short_queries() {
        let mut conn = test_db();
        let feed = db::upsert_feed(&conn, "https://x.example/feed", "テックブログ", None).unwrap();
        seed_article(&mut conn, feed, "Rustの非同期ランタイム入門", 1);
        seed_article(&mut conn, feed, "SwiftUIでアニメーション", 2);

        // trigram FTS (3+ chars)
        let hits = db::search_articles(&conn, "ランタイム", 50).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].title.contains("Rust"));

        // short query falls back to LIKE
        let hits = db::search_articles(&conn, "Sw", 50).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].title.contains("SwiftUI"));

        // AND of terms
        let hits = db::search_articles(&conn, "Rust 非同期", 50).unwrap();
        assert_eq!(hits.len(), 1);
        let hits = db::search_articles(&conn, "Rust アニメーション", 50).unwrap();
        assert_eq!(hits.len(), 0);

        // On-demand page extraction is added to the same FTS index.
        let rust_id = db::search_articles(&conn, "ランタイム", 50).unwrap()[0].id;
        db::store_full_text(&conn, rust_id, "本文だけに登場するカモノハシ識別子").unwrap();
        let hits = db::search_articles(&conn, "カモノハシ", 50).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, rust_id);

        db::store_ai_summary(&conn, rust_id, "AIによる要約", "provider/model").unwrap();
        let article = db::get_article(&conn, rust_id).unwrap();
        assert_eq!(article.ai_summary.as_deref(), Some("AIによる要約"));
        assert_eq!(article.ai_summary_model.as_deref(), Some("provider/model"));
    }

    #[test]
    fn cleanup_keeps_unread_and_starred() {
        let mut conn = test_db();
        let feed = db::upsert_feed(&conn, "https://y.example/feed", "blog", None).unwrap();
        seed_article(&mut conn, feed, "old-read", 100);
        seed_article(&mut conn, feed, "old-starred", 100);
        seed_article(&mut conn, feed, "old-unread", 100);
        seed_article(&mut conn, feed, "new-read", 1);

        let ids: Vec<(i64, String)> = conn
            .prepare("SELECT id, title FROM articles")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for (id, title) in &ids {
            if title != "old-unread" {
                db::set_read(&conn, *id, true).unwrap();
            }
            if title == "old-starred" {
                db::set_starred(&conn, *id, true).unwrap();
            }
        }

        let deleted = db::cleanup_old_articles(&conn, 90).unwrap();
        assert_eq!(deleted, 1); // only old-read

        let remaining: Vec<String> = conn
            .prepare("SELECT title FROM articles ORDER BY title")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(remaining, vec!["new-read", "old-starred", "old-unread"]);

        // FTS index stays in sync via triggers.
        let hits = db::search_articles(&conn, "old-read", 50).unwrap();
        assert_eq!(hits.len(), 0);

        // A feed that still advertises the cleaned-up item must not resurrect
        // it as unread; the lightweight read-history tombstone blocks it.
        seed_article(&mut conn, feed, "old-read", 100);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles WHERE title = 'old-read'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn parses_folders_as_categories() {
        let opml = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="1.0">
  <body>
    <outline text="Dev" title="Dev">
      <outline text="Blog A" title="Blog A" type="rss" xmlUrl="https://a.example/feed" htmlUrl="https://a.example/"/>
      <outline text="Blog &amp; B" title="Blog &amp; B" type="rss" xmlUrl="https://b.example/feed?a=1&amp;b=2"/>
    </outline>
    <outline text="Top" title="Top" type="rss" xmlUrl="https://top.example/feed"/>
    <outline text="Outer" title="Outer">
      <outline text="Inner" title="Inner">
        <outline text="Nested" title="Nested" type="rss" xmlUrl="https://n.example/feed"/>
      </outline>
    </outline>
  </body>
</opml>"#;
        let feeds = parse_opml(opml);
        assert_eq!(feeds.len(), 4);
        assert_eq!(feeds[0].title, "Blog A");
        assert_eq!(feeds[0].category.as_deref(), Some("Dev"));
        assert_eq!(feeds[1].title, "Blog & B");
        assert_eq!(feeds[1].url, "https://b.example/feed?a=1&b=2");
        assert_eq!(feeds[1].category.as_deref(), Some("Dev"));
        assert_eq!(feeds[2].title, "Top");
        assert_eq!(feeds[2].category, None);
        assert_eq!(feeds[3].category.as_deref(), Some("Inner"));
    }
}
