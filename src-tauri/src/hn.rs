//! Hacker News モジュール。
//!
//! RSSとは独立した情報ソース。Algolia の HN API からフロントページを取得して
//! `hn_items` に保存し、digest エンジンで日本語タイトル・本文ダイジェスト・
//! コメント要約を生成する。UIも専用ビューを持つ。

use crate::{db, digest, fetcher};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

const ITEM_TYPE: &str = "hn";
const DEFAULT_DIGEST_MODEL: &str = "openai-codex/gpt-5.6-luna";
const FRONT_PAGE_API: &str = "https://hn.algolia.com/api/v1/search?tags=front_page&hitsPerPage=30";
const BATCH_SIZE: i64 = 4;
// 同時に走らせるバッチ数（= pi の並列プロセス数）
const PARALLEL_BATCHES: usize = 3;
const MAX_ROUNDS: usize = 20;
const BODY_MAX_CHARS: usize = 6000;
const COMMENTS_MAX_CHARS: usize = 5000;

/// Only one digest run at a time; refresh cycles just skip if one is active.
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone)]
pub struct HnItem {
    pub id: i64,
    pub title: String,
    pub url: Option<String>,
    pub comments_url: String,
    pub points: i64,
    pub comments_count: i64,
    pub rank: i64,
    pub digest: Option<digest::Digest>,
}

// ---------------------------------------------------------------------------
// storage
// ---------------------------------------------------------------------------

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS hn_items (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            url TEXT,
            points INTEGER NOT NULL DEFAULT 0,
            comments_count INTEGER NOT NULL DEFAULT 0,
            rank INTEGER NOT NULL DEFAULT 0,
            fetched_at INTEGER
        );
        "#,
    )
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<HnItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, url, points, comments_count, rank
         FROM hn_items ORDER BY rank",
    )?;
    let rows = stmt.query_map([], |r| {
        let id: i64 = r.get(0)?;
        Ok(HnItem {
            id,
            title: r.get(1)?,
            url: r.get(2)?,
            comments_url: format!("https://news.ycombinator.com/item?id={id}"),
            points: r.get(3)?,
            comments_count: r.get(4)?,
            rank: r.get(5)?,
            digest: None,
        })
    })?;
    let mut items: Vec<HnItem> = rows.collect::<Result<_, _>>()?;
    let ids: Vec<i64> = items.iter().map(|i| i.id).collect();
    let mut digests = digest::digests_for(conn, ITEM_TYPE, &ids)?;
    for item in &mut items {
        item.digest = digests.remove(&item.id);
    }
    Ok(items)
}

fn selected_model(conn: &Connection) -> String {
    db::get_setting(conn, "translate_model")
        .ok()
        .flatten()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DIGEST_MODEL.to_string())
}

fn count_pending(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM hn_items h
         LEFT JOIN digests d ON d.item_type = 'hn' AND d.item_id = h.id
         WHERE d.item_id IS NULL",
        [],
        |r| r.get(0),
    )
}

struct Pending {
    id: i64,
    title: String,
    url: Option<String>,
    comments_count: i64,
}

fn pending_items(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<Pending>> {
    let mut stmt = conn.prepare(
        "SELECT h.id, h.title, h.url, h.comments_count FROM hn_items h
         LEFT JOIN digests d ON d.item_type = 'hn' AND d.item_id = h.id
         WHERE d.item_id IS NULL ORDER BY h.rank LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(Pending {
            id: r.get(0)?,
            title: r.get(1)?,
            url: r.get(2)?,
            comments_count: r.get(3)?,
        })
    })?;
    rows.collect()
}

// ---------------------------------------------------------------------------
// front page fetch
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AlgoliaResponse {
    hits: Vec<AlgoliaHit>,
}

#[derive(Deserialize)]
struct AlgoliaHit {
    #[serde(rename = "objectID")]
    object_id: String,
    title: Option<String>,
    url: Option<String>,
    #[serde(default)]
    points: Option<i64>,
    #[serde(default)]
    num_comments: Option<i64>,
}

/// Fetch the current front page and replace the stored items.
/// Digests persist across refreshes (keyed by HN item id).
pub async fn refresh(app: &AppHandle) -> Result<usize, String> {
    let client = {
        let state = app.state::<crate::AppState>();
        state.client.clone()
    };
    let body = client
        .get(FRONT_PAGE_API)
        .send()
        .await
        .map_err(|e| format!("Hacker Newsを取得できません: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Hacker Newsを取得できません: {e}"))?;
    let res: AlgoliaResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Hacker Newsの応答を解析できません: {e}"))?;

    let count = {
        let state = app.state::<crate::AppState>();
        let conn = crate::lock_db(&state)?;
        conn.execute("DELETE FROM hn_items", [])
            .map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO hn_items (id, title, url, points, comments_count, rank, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%s','now'))",
            )
            .map_err(|e| e.to_string())?;
        let mut rank: i64 = 0;
        for hit in &res.hits {
            let Ok(id) = hit.object_id.parse::<i64>() else {
                continue;
            };
            let Some(title) = hit.title.as_deref().filter(|t| !t.is_empty()) else {
                continue;
            };
            rank += 1;
            stmt.execute(params![
                id,
                title,
                hit.url,
                hit.points.unwrap_or(0),
                hit.num_comments.unwrap_or(0),
                rank,
            ])
            .map_err(|e| e.to_string())?;
        }
        rank
    };
    let _ = app.emit("hn-updated", ());
    kick(app);
    Ok(count as usize)
}

// ---------------------------------------------------------------------------
// digest pipeline
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
struct HnStatus {
    active: bool,
    remaining: i64,
}

fn emit_status(app: &AppHandle, active: bool) {
    let remaining = (|| -> Result<i64, String> {
        let state = app.state::<crate::AppState>();
        let conn = crate::lock_db(&state)?;
        count_pending(&conn).map_err(|e| e.to_string())
    })()
    .unwrap_or(0);
    let _ = app.emit("hn-status", HnStatus { active, remaining });
}

/// Start digesting pending items in the background (no-op if already running).
pub fn kick(app: &AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run(&app).await;
        RUNNING.store(false, Ordering::SeqCst);
        emit_status(&app, false);
        if let Err(e) = result {
            eprintln!("Hacker News digest failed: {e}");
            let _ = app.emit("hn-error", &e);
        }
    });
}

async fn run(app: &AppHandle) -> Result<(), String> {
    let mut stalled_rounds = 0;
    let mut last_error: Option<String> = None;

    for _ in 0..MAX_ROUNDS {
        let (pending, remaining_before, model, client) = {
            let state = app.state::<crate::AppState>();
            let conn = crate::lock_db(&state)?;
            let pending = pending_items(&conn, BATCH_SIZE * PARALLEL_BATCHES as i64)
                .map_err(|e| e.to_string())?;
            let remaining = count_pending(&conn).map_err(|e| e.to_string())?;
            let model = Some(selected_model(&conn));
            (pending, remaining, model, state.client.clone())
        };
        if pending.is_empty() {
            return Ok(());
        }
        emit_status(app, true);

        // 各バッチを独立に処理する（本文取得→翻訳→保存）。piも並列に走る
        let mut set = tokio::task::JoinSet::new();
        let mut chunks: Vec<Vec<Pending>> = Vec::new();
        let mut it = pending.into_iter().peekable();
        while it.peek().is_some() {
            chunks.push(it.by_ref().take(BATCH_SIZE as usize).collect());
        }
        for chunk in chunks {
            let app = app.clone();
            let model = model.clone();
            let client = client.clone();
            set.spawn(async move { process_batch(&app, chunk, model, client).await });
        }
        let mut round_error: Option<String> = None;
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Err(e)) => {
                    round_error.get_or_insert(e);
                }
                Err(e) => {
                    round_error.get_or_insert_with(|| format!("翻訳タスクが終了しました: {e}"));
                }
                Ok(Ok(())) => {}
            }
        }

        let remaining_after = {
            let state = app.state::<crate::AppState>();
            let conn = crate::lock_db(&state)?;
            count_pending(&conn).map_err(|e| e.to_string())?
        };
        emit_status(app, true);
        if remaining_after == 0 {
            return Ok(());
        }

        if let Some(e) = round_error {
            last_error = Some(e);
            if remaining_after >= remaining_before {
                stalled_rounds += 1;
                if stalled_rounds >= 3 {
                    return Err(last_error.unwrap());
                }
                // Transient provider/network errors should not strand the last
                // few stories until the next 15-minute refresh.
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            } else {
                stalled_rounds = 0;
            }
        } else {
            stalled_rounds = 0;
        }
    }

    Err(last_error.unwrap_or_else(|| "翻訳キューを時間内に処理できませんでした".to_string()))
}

async fn process_batch(
    app: &AppHandle,
    batch: Vec<Pending>,
    model: Option<String>,
    client: reqwest::Client,
) -> Result<(), String> {
    // fetch linked article bodies and comment threads concurrently;
    // a failed fetch just means that part is missing from the digest
    let mut fetched: Vec<(Option<String>, Option<String>)> = Vec::new();
    {
        let mut set = tokio::task::JoinSet::new();
        for (i, item) in batch.iter().enumerate() {
            let client = client.clone();
            let url = item.url.clone();
            let comments_url = (item.comments_count > 0)
                .then(|| format!("https://news.ycombinator.com/item?id={}", item.id));
            set.spawn(async move {
                let body = match url {
                    Some(u) => fetcher::fetch_page_text(&client, &u, BODY_MAX_CHARS)
                        .await
                        .ok(),
                    None => None,
                };
                let comments = match comments_url {
                    Some(u) => fetcher::fetch_page_text(&client, &u, COMMENTS_MAX_CHARS)
                        .await
                        .ok(),
                    None => None,
                };
                (i, body, comments)
            });
        }
        fetched.resize(batch.len(), (None, None));
        while let Some(Ok((i, body, comments))) = set.join_next().await {
            fetched[i] = (body, comments);
        }
    }

    let items: Vec<digest::BatchItem> = batch
        .iter()
        .zip(&fetched)
        .map(|(it, (body, comments))| digest::BatchItem {
            id: it.id,
            title: it.title.clone(),
            fallback_summary: None,
            body: body.clone(),
            comments: comments.clone(),
        })
        .collect();
    let translated = digest::translate_batch(&items, model.as_deref()).await?;
    if translated.is_empty() {
        return Err("翻訳結果を解析できませんでした".to_string());
    }

    {
        let state = app.state::<crate::AppState>();
        let conn = crate::lock_db(&state)?;
        let valid: std::collections::HashSet<i64> = batch.iter().map(|b| b.id).collect();
        for t in &translated {
            if !valid.contains(&t.id) || t.title_ja.trim().is_empty() {
                continue; // don't let a hallucinated id touch other rows
            }
            let summary = t.summary_ja.as_deref().filter(|s| !s.trim().is_empty());
            digest::store(&conn, ITEM_TYPE, t.id, t.title_ja.trim(), summary)
                .map_err(|e| e.to_string())?;
            if let Some(c) = t
                .comments_summary_ja
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                digest::store_comments_summary(&conn, ITEM_TYPE, t.id, c.trim())
                    .map_err(|e| e.to_string())?;
            }
        }
        // items the model skipped would be re-requested forever; mark them
        // as digested with the original title so the queue drains
        let returned: std::collections::HashSet<i64> = translated.iter().map(|t| t.id).collect();
        for item in &batch {
            if !returned.contains(&item.id) {
                let _ = digest::store(&conn, ITEM_TYPE, item.id, &item.title, None);
            }
        }
    }
    let _ = app.emit("hn-updated", ());
    Ok(())
}

/// Summarize the HN comments thread in Japanese. Cached per item.
pub async fn summarize_comments(app: &AppHandle, item_id: i64) -> Result<String, String> {
    let (url, model, client) = {
        let state = app.state::<crate::AppState>();
        let conn = crate::lock_db(&state)?;
        if let Some(cached) = digest::digests_for(&conn, ITEM_TYPE, &[item_id])
            .map_err(|e| e.to_string())?
            .remove(&item_id)
            .and_then(|d| d.comments_summary_ja)
        {
            return Ok(cached);
        }
        (
            format!("https://news.ycombinator.com/item?id={item_id}"),
            Some(selected_model(&conn)),
            state.client.clone(),
        )
    };

    let text = fetcher::fetch_page_text(&client, &url, 12000).await?;
    let summary = digest::summarize_comments_text(&text, model.as_deref()).await?;

    let state = app.state::<crate::AppState>();
    let conn = crate::lock_db(&state)?;
    digest::store_comments_summary(&conn, ITEM_TYPE, item_id, &summary)
        .map_err(|e| e.to_string())?;
    Ok(summary)
}
