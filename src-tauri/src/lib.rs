mod db;
mod fetcher;
mod pi_bridge;
mod search;

use db::{Article, Feed};
use pi_bridge::PiBridge;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    db: Mutex<Connection>,
    client: reqwest::Client,
    pi: PiBridge,
}

#[derive(Serialize)]
struct RefreshResult {
    new_articles: usize,
    failed: Vec<String>,
}

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
    state.db.lock().map_err(|_| "DBロックに失敗しました".to_string())
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
    unread_only: bool,
    starred_only: bool,
) -> Result<Vec<Article>, String> {
    let conn = lock_db(&state)?;
    db::list_articles(&conn, feed_id, unread_only, starred_only, 500).map_err(|e| e.to_string())
}

#[tauri::command]
fn fuzzy_search(state: State<AppState>, query: String) -> Result<Vec<Article>, String> {
    let conn = lock_db(&state)?;
    let corpus = db::search_corpus(&conn).map_err(|e| e.to_string())?;
    let ids = search::fuzzy_rank(&corpus, &query, 50);
    db::articles_by_ids(&conn, &ids).map_err(|e| e.to_string())
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
fn mark_all_read(state: State<AppState>, feed_id: Option<i64>) -> Result<(), String> {
    let conn = lock_db(&state)?;
    db::mark_all_read(&conn, feed_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_feed(state: State<AppState>, feed_id: i64) -> Result<(), String> {
    let conn = lock_db(&state)?;
    db::remove_feed(&conn, feed_id).map_err(|e| e.to_string())
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
            && (tag_lower.contains("application/rss+xml") || tag_lower.contains("application/atom+xml"))
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
            let res = client.get(&url).send().await.map_err(|_| first_err.clone())?;
            let html = res.text().await.map_err(|_| first_err.clone())?;
            let discovered = discover_feed_url(&html, &url).ok_or(first_err)?;
            let fetched = fetcher::fetch_feed(&client, &discovered).await?;
            (discovered, fetched)
        }
    };

    let feed = {
        let state = app.state::<AppState>();
        let mut conn = lock_db(&state)?;
        let feed_id = db::upsert_feed(&conn, &feed_url, &fetched.title, fetched.site_url.as_deref())
            .map_err(|e| e.to_string())?;
        db::insert_articles(&mut conn, feed_id, &fetched.articles).map_err(|e| e.to_string())?;
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
            let Ok((feed_id, url, result)) = joined else { continue };
            match result {
                Ok(fetched) => {
                    let mut conn = lock_db(&state)?;
                    let _ = db::upsert_feed(&conn, &url, &fetched.title, fetched.site_url.as_deref());
                    match db::insert_articles(&mut conn, feed_id, &fetched.articles) {
                        Ok(n) => new_articles += n,
                        Err(e) => failed.push(format!("{url}: {e}")),
                    }
                }
                Err(e) => failed.push(format!("{url}: {e}")),
            }
        }
        // let the UI update progressively during large refreshes
        let _ = app.emit("feeds-updated", ());
    }
    let _ = app.emit("feeds-updated", ());
    Ok(RefreshResult { new_articles, failed })
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

/// Import feeds from OPML text. Returns the number of newly registered feeds;
/// fetching happens in the background afterwards.
#[tauri::command]
async fn import_opml(app: AppHandle, content: String) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let mut added = 0;
    {
        let conn = lock_db(&state)?;
        let lower = content.to_lowercase();
        let mut pos = 0;
        while let Some(start) = lower[pos..].find("<outline") {
            let start = pos + start;
            let Some(end) = lower[start..].find('>').map(|e| start + e) else { break };
            let tag = &content[start..=end];
            if let Some(xml_url) = extract_attr(tag, "xmlurl") {
                let title = extract_attr(tag, "title")
                    .or_else(|| extract_attr(tag, "text"))
                    .unwrap_or_else(|| xml_url.clone());
                let url = xml_unescape(&xml_url);
                let title = xml_unescape(&title);
                if db::insert_feed_stub(&conn, &url, &title).map_err(|e| e.to_string())? {
                    added += 1;
                }
            }
            pos = end + 1;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::open(&data_dir.join("myfocus.db"))?;

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?;

            app.manage(AppState {
                db: Mutex::new(conn),
                client,
                pi: PiBridge::new(),
            });

            // background poller: refresh on launch, then every 15 minutes
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let _ = refresh_all_inner(&handle).await;
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_feeds,
            list_articles,
            fuzzy_search,
            mark_read,
            mark_starred,
            mark_all_read,
            remove_feed,
            add_feed,
            refresh_all,
            import_opml,
            ai_prompt,
            ai_abort,
            ai_new_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
