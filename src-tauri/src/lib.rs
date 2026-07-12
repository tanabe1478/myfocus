mod db;
mod fetcher;
mod pi_bridge;
mod translator;

use db::{Article, Feed};
use pi_bridge::PiBridge;
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

pub(crate) struct AppState {
    pub(crate) db: Mutex<Connection>,
    pub(crate) client: reqwest::Client,
    pi: PiBridge,
}

#[derive(Serialize)]
struct RefreshResult {
    new_articles: usize,
    failed: Vec<String>,
}

pub(crate) fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
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
    category: Option<String>,
    unread_only: bool,
    starred_only: bool,
) -> Result<Vec<Article>, String> {
    let conn = lock_db(&state)?;
    db::list_articles(&conn, feed_id, category.as_deref(), unread_only, starred_only)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_article(state: State<AppState>, article_id: i64) -> Result<Article, String> {
    let conn = lock_db(&state)?;
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
fn set_setting(state: State<AppState>, key: String, value: String) -> Result<(), String> {
    let conn = lock_db(&state)?;
    db::set_setting(&conn, &key, &value).map_err(|e| e.to_string())
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

#[tauri::command]
fn set_feed_translate(app: AppHandle, feed_id: i64, translate: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        db::set_feed_translate(&conn, feed_id, translate).map_err(|e| e.to_string())?;
    }
    if translate {
        translator::kick(&app);
    }
    Ok(())
}

/// Summarize the discussion at the article's comments URL (HN thread etc.) in
/// Japanese. Cached per article; the first call generates it.
#[tauri::command]
async fn summarize_comments(app: AppHandle, article_id: i64) -> Result<String, String> {
    let (target, model, client) = {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        let article = db::get_article(&conn, article_id).map_err(|e| e.to_string())?;
        if let Some(cached) = article.comments_summary_ja {
            return Ok(cached);
        }
        let target = article
            .comments_url
            .or(article.url)
            .ok_or("この記事にはコメントページのURLがありません")?;
        let model = db::get_setting(&conn, "translate_model").ok().flatten();
        (target, model, state.client.clone())
    };

    let text = fetcher::fetch_page_text(&client, &target, 12000).await?;
    let prompt = format!(
        "以下はある記事に対するコメントスレッド（またはコメントを含むページ）のテキストです。\
         議論の主な論点、目立った意見、意見の対立があればその両論を、日本語で3〜6項目の箇条書きに要約してください。\
         各項目は「- 」で始めること。前置きや締めの文は書かないこと。\
         コメントが見つからない場合は「コメントはまだ無いようです。」とだけ返すこと。\n\n{text}"
    );
    let summary = translator::pi_print(&prompt, model.as_deref()).await?;
    let summary = summary.trim().to_string();
    if summary.is_empty() {
        return Err("要約を生成できませんでした".to_string());
    }

    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    db::store_comments_summary(&conn, article_id, &summary).map_err(|e| e.to_string())?;
    Ok(summary)
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
    // start on any backlog immediately; new articles are picked up by the
    // second kick after the refresh completes
    translator::kick(app);
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
    // translate any new articles on translate-enabled feeds
    translator::kick(app);
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
                let Some(end) = lower[o..].find('>').map(|e| o + e) else { break };
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
/// `open -g` asks for no activation, but some browsers (Chrome etc.) activate
/// themselves anyway — so we also grab the focus back right afterwards.
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
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("この機能はmacOSのみ対応です".to_string())
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
        let _ = db::cleanup_old_articles(&conn, days);
    }
}

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
                    run_cleanup(&handle);
                    let _ = refresh_all_inner(&handle).await;
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_feeds,
            list_articles,
            get_article,
            fuzzy_search,
            get_setting,
            set_setting,
            mark_read,
            mark_starred,
            mark_all_read,
            remove_feed,
            set_feed_translate,
            summarize_comments,
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
                comments_url: None,
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

        // FTS index stays in sync via triggers
        let hits = db::search_articles(&conn, "old-read", 50).unwrap();
        assert_eq!(hits.len(), 0);
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
