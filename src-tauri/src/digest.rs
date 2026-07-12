//! 日本語ダイジェスト（翻訳・要約）機能。
//!
//! RSSとは独立した「コンテンツへの付加情報」レイヤー。ダイジェストは
//! `digests` テーブルに item_type + item_id で保存され、RSS記事以外の
//! ソースにも将来対応できる。対象ソースの選択は `digest_rules` が持つ。
//! 依存方向は digest → RSS（記事の読み取り）のみで、RSS側はこの
//! モジュールを知らない。

use crate::{db, fetcher};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Only one digest run at a time; refresh cycles just skip if one is active.
static RUNNING: AtomicBool = AtomicBool::new(false);

// bodies make prompts heavy, so batches stay small
const BATCH_SIZE: i64 = 4;
// safety cap per run: a large backlog continues on the next refresh cycle
const MAX_BATCHES: usize = 30;
const BODY_MAX_CHARS: usize = 6000;
const PI_TIMEOUT: Duration = Duration::from_secs(300);

const ITEM_TYPE_ARTICLE: &str = "article";

#[derive(Serialize, Clone)]
pub struct Digest {
    pub title_ja: Option<String>,
    pub summary_ja: Option<String>,
    pub comments_summary_ja: Option<String>,
}

// ---------------------------------------------------------------------------
// schema & migration
// ---------------------------------------------------------------------------

/// Create digest tables and migrate data from the legacy columns that used to
/// live on the RSS `articles` / `feeds` tables.
pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS digests (
            item_type TEXT NOT NULL DEFAULT 'article',
            item_id INTEGER NOT NULL,
            title_ja TEXT,
            summary_ja TEXT,
            comments_summary_ja TEXT,
            updated_at INTEGER,
            PRIMARY KEY (item_type, item_id)
        );

        CREATE TABLE IF NOT EXISTS digest_rules (
            source_type TEXT NOT NULL DEFAULT 'feed',
            source_id INTEGER NOT NULL,
            PRIMARY KEY (source_type, source_id)
        );
        "#,
    )?;

    let has_column = |table: &str, column: &str| -> rusqlite::Result<bool> {
        conn.prepare(&format!(
            "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"
        ))?
        .exists([column])
    };

    if has_column("articles", "title_ja")? {
        conn.execute(
            "INSERT OR IGNORE INTO digests (item_type, item_id, title_ja, summary_ja, comments_summary_ja, updated_at)
             SELECT 'article', id, title_ja, summary_ja, comments_summary_ja, strftime('%s','now')
             FROM articles
             WHERE title_ja IS NOT NULL OR summary_ja IS NOT NULL OR comments_summary_ja IS NOT NULL",
            [],
        )?;
        for col in ["title_ja", "summary_ja", "comments_summary_ja"] {
            conn.execute(&format!("ALTER TABLE articles DROP COLUMN {col}"), [])?;
        }
    }

    if has_column("feeds", "translate")? {
        conn.execute(
            "INSERT OR IGNORE INTO digest_rules (source_type, source_id)
             SELECT 'feed', id FROM feeds WHERE translate = 1",
            [],
        )?;
        conn.execute("ALTER TABLE feeds DROP COLUMN translate", [])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// rules & lookups
// ---------------------------------------------------------------------------

pub fn set_rule(conn: &Connection, feed_id: i64, enabled: bool) -> rusqlite::Result<()> {
    if enabled {
        conn.execute(
            "INSERT OR IGNORE INTO digest_rules (source_type, source_id) VALUES ('feed', ?1)",
            [feed_id],
        )?;
    } else {
        conn.execute(
            "DELETE FROM digest_rules WHERE source_type = 'feed' AND source_id = ?1",
            [feed_id],
        )?;
    }
    Ok(())
}

/// Feed ids that are opted into Japanese digestion.
pub fn list_rules(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut stmt =
        conn.prepare("SELECT source_id FROM digest_rules WHERE source_type = 'feed'")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// Digests for the given article ids (missing ids are simply absent).
pub fn digests_for(conn: &Connection, ids: &[i64]) -> rusqlite::Result<HashMap<i64, Digest>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT item_id, title_ja, summary_ja, comments_summary_ja
         FROM digests WHERE item_type = '{ITEM_TYPE_ARTICLE}' AND item_id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            Digest {
                title_ja: r.get(1)?,
                summary_ja: r.get(2)?,
                comments_summary_ja: r.get(3)?,
            },
        ))
    })?;
    rows.collect()
}

fn store(
    conn: &Connection,
    article_id: i64,
    title_ja: &str,
    summary_ja: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO digests (item_type, item_id, title_ja, summary_ja, updated_at)
         VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))
         ON CONFLICT(item_type, item_id)
         DO UPDATE SET title_ja = excluded.title_ja, summary_ja = excluded.summary_ja,
                       updated_at = excluded.updated_at",
        params![ITEM_TYPE_ARTICLE, article_id, title_ja, summary_ja],
    )?;
    Ok(())
}

fn store_comments_summary(conn: &Connection, article_id: i64, summary: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO digests (item_type, item_id, comments_summary_ja, updated_at)
         VALUES (?1, ?2, ?3, strftime('%s','now'))
         ON CONFLICT(item_type, item_id)
         DO UPDATE SET comments_summary_ja = excluded.comments_summary_ja,
                       updated_at = excluded.updated_at",
        params![ITEM_TYPE_ARTICLE, article_id, summary],
    )?;
    Ok(())
}

/// Drop digests whose source article no longer exists (retention cleanup,
/// feed removal). Called at the start of each run.
fn gc(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM digests
         WHERE item_type = 'article' AND item_id NOT IN (SELECT id FROM articles)",
        [],
    )
}

struct PendingItem {
    id: i64,
    title: String,
    url: Option<String>,
    summary: Option<String>,
}

/// Newest articles from opted-in feeds that don't have a digest yet.
fn pending_items(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<PendingItem>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.title, a.url, a.summary
         FROM articles a
         JOIN digest_rules r ON r.source_type = 'feed' AND r.source_id = a.feed_id
         LEFT JOIN digests d ON d.item_type = 'article' AND d.item_id = a.id
         WHERE d.item_id IS NULL AND a.title != ''
         ORDER BY a.published_at DESC, a.id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok(PendingItem {
            id: row.get(0)?,
            title: row.get(1)?,
            url: row.get(2)?,
            summary: row.get(3)?,
        })
    })?;
    rows.collect()
}

fn count_pending(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM articles a
         JOIN digest_rules r ON r.source_type = 'feed' AND r.source_id = a.feed_id
         LEFT JOIN digests d ON d.item_type = 'article' AND d.item_id = a.id
         WHERE d.item_id IS NULL AND a.title != ''",
        [],
        |r| r.get(0),
    )
}

// ---------------------------------------------------------------------------
// background pipeline
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TranslatedItem {
    id: i64,
    title_ja: String,
    #[serde(default)]
    summary_ja: Option<String>,
}

#[derive(Serialize, Clone)]
struct DigestStatus {
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
    let _ = app.emit("translate-status", DigestStatus { active, remaining });
}

/// Start digesting pending articles in the background (no-op if already running).
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
            let _ = app.emit("translate-error", &e);
        }
    });
}

async fn run(app: &AppHandle) -> Result<(), String> {
    {
        let state = app.state::<crate::AppState>();
        let conn = crate::lock_db(&state)?;
        let _ = gc(&conn);
    }
    for _ in 0..MAX_BATCHES {
        let (batch, model, client) = {
            let state = app.state::<crate::AppState>();
            let conn = crate::lock_db(&state)?;
            let batch = pending_items(&conn, BATCH_SIZE).map_err(|e| e.to_string())?;
            let model = db::get_setting(&conn, "translate_model").ok().flatten();
            (batch, model, state.client.clone())
        };
        if batch.is_empty() {
            return Ok(());
        }
        emit_status(app, true);

        // fetch linked article bodies concurrently; a failed fetch just means
        // the digest falls back to the RSS summary
        let mut bodies: Vec<Option<String>> = Vec::with_capacity(batch.len());
        {
            let mut set = tokio::task::JoinSet::new();
            for (i, item) in batch.iter().enumerate() {
                let client = client.clone();
                let url = item.url.clone();
                set.spawn(async move {
                    let body = match url {
                        Some(u) => fetcher::fetch_page_text(&client, &u, BODY_MAX_CHARS).await.ok(),
                        None => None,
                    };
                    (i, body)
                });
            }
            bodies.resize(batch.len(), None);
            while let Some(Ok((i, body))) = set.join_next().await {
                bodies[i] = body;
            }
        }

        let translated = digest_batch(&batch, &bodies, model.as_deref()).await?;
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
                store(&conn, t.id, t.title_ja.trim(), summary).map_err(|e| e.to_string())?;
            }
            // items the model skipped would be re-requested forever; mark them
            // as digested with the original title so the queue drains
            let returned: std::collections::HashSet<i64> = translated.iter().map(|t| t.id).collect();
            for item in &batch {
                if !returned.contains(&item.id) {
                    let _ = store(&conn, item.id, &item.title, None);
                }
            }
        }
        let _ = app.emit("feeds-updated", ());
    }
    Ok(())
}

async fn digest_batch(
    batch: &[PendingItem],
    bodies: &[Option<String>],
    model: Option<&str>,
) -> Result<Vec<TranslatedItem>, String> {
    let items: Vec<serde_json::Value> = batch
        .iter()
        .zip(bodies)
        .map(|(it, body)| {
            json!({
                "id": it.id,
                "title": it.title,
                "rss_summary": it.summary.as_deref().map(|s| s.chars().take(400).collect::<String>()),
                "body": body,
            })
        })
        .collect();

    let prompt = format!(
        r#"あなたは翻訳・要約エンジンです。以下のJSON配列の各記事について:

- "title_ja": 自然で簡潔な日本語タイトル。意訳してよいが、固有名詞・プロダクト名は原語のまま残す。すでに日本語のタイトルはそのまま返す。
- "summary_ja": 記事を読まなくても内容が理解できる日本語ダイジェスト。
  - "body"がある場合: その本文に基づいて2〜3段落（300〜500字程度）で。要点・背景・なぜ興味深いかを含める。段落は\n\nで区切る。
  - "body"がnullで"rss_summary"がある場合: それを1〜2文の日本語に。
  - どちらも無い場合: null。
  - 本文の内容だけを根拠にし、推測で補わないこと。

出力は入力と同じ"id"を持つJSON配列のみ。コードフェンス・説明文・前置きは一切出力しないこと。

{}"#,
        serde_json::to_string(&items).map_err(|e| e.to_string())?
    );

    let output = pi_print(&prompt, model).await?;
    parse_translations(&output)
}

/// Summarize the discussion at the article's comments URL (HN thread etc.) in
/// Japanese. Cached in the digests table; the first call generates it.
pub async fn summarize_comments(app: &AppHandle, article_id: i64) -> Result<String, String> {
    let (target, model, client) = {
        let state = app.state::<crate::AppState>();
        let conn = crate::lock_db(&state)?;
        if let Some(cached) = digests_for(&conn, &[article_id])
            .map_err(|e| e.to_string())?
            .remove(&article_id)
            .and_then(|d| d.comments_summary_ja)
        {
            return Ok(cached);
        }
        let article = db::get_article(&conn, article_id).map_err(|e| e.to_string())?;
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
    let summary = pi_print(&prompt, model.as_deref()).await?;
    let summary = summary.trim().to_string();
    if summary.is_empty() {
        return Err("要約を生成できませんでした".to_string());
    }

    let state = app.state::<crate::AppState>();
    let conn = crate::lock_db(&state)?;
    store_comments_summary(&conn, article_id, &summary).map_err(|e| e.to_string())?;
    Ok(summary)
}

/// Run `pi -p` with all agentic features disabled and return its text output.
/// Used for stateless jobs (translation, summarization) so they don't touch
/// the chat assistant's RPC session.
async fn pi_print(prompt: &str, model: Option<&str>) -> Result<String, String> {
    let mut args: Vec<&str> = vec![
        "-p",
        "--no-tools",
        "--no-session",
        "--no-context-files",
        "--no-extensions",
        "--no-skills",
        "--thinking",
        "off",
    ];
    if let Some(m) = model {
        args.push("--model");
        args.push(m);
    }
    args.push(prompt);

    let output = tokio::time::timeout(
        PI_TIMEOUT,
        tokio::process::Command::new("pi")
            .env("PATH", crate::pi_bridge::login_shell_path())
            .args(&args)
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| "piの応答がタイムアウトしました".to_string())?
    .map_err(|e| format!("piを起動できません: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "piの実行が失敗しました: {}",
            err.lines().last().unwrap_or("不明なエラー")
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Extract the JSON array from model output, tolerating stray text around it.
fn parse_translations(text: &str) -> Result<Vec<TranslatedItem>, String> {
    let start = text.find('[').ok_or("応答にJSON配列がありません")?;
    let end = text.rfind(']').ok_or("応答にJSON配列がありません")?;
    if end <= start {
        return Err("応答のJSONが壊れています".to_string());
    }
    serde_json::from_str(&text[start..=end]).map_err(|e| format!("JSON解析エラー: {e}"))
}

#[cfg(test)]
mod tests {
    use super::parse_translations;

    #[test]
    fn init_migrates_legacy_columns() {
        let dir = std::env::temp_dir().join(format!("myfocus-digest-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("legacy.db");
        let _ = std::fs::remove_file(&path);
        let mut conn = crate::db::open(&path).unwrap();

        // simulate the pre-separation schema with data
        conn.execute_batch(
            "ALTER TABLE feeds ADD COLUMN translate INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE articles ADD COLUMN title_ja TEXT;
             ALTER TABLE articles ADD COLUMN summary_ja TEXT;
             ALTER TABLE articles ADD COLUMN comments_summary_ja TEXT;",
        )
        .unwrap();
        let feed = crate::db::upsert_feed(&conn, "https://l.example/feed", "legacy", None).unwrap();
        conn.execute("UPDATE feeds SET translate = 1 WHERE id = ?1", [feed]).unwrap();
        crate::db::insert_articles(
            &mut conn,
            feed,
            &[crate::db::NewArticle {
                guid: "g1".into(),
                title: "Original".into(),
                url: None,
                author: None,
                summary: None,
                content_html: None,
                published_at: Some(1),
                comments_url: None,
            }],
        )
        .unwrap();
        conn.execute("UPDATE articles SET title_ja = '訳', summary_ja = '要約'", []).unwrap();

        super::init(&conn).unwrap();

        // data moved to the digest tables, legacy columns dropped
        let rules = super::list_rules(&conn).unwrap();
        assert_eq!(rules, vec![feed]);
        let id: i64 = conn.query_row("SELECT id FROM articles", [], |r| r.get(0)).unwrap();
        let digests = super::digests_for(&conn, &[id]).unwrap();
        assert_eq!(digests[&id].title_ja.as_deref(), Some("訳"));
        assert_eq!(digests[&id].summary_ja.as_deref(), Some("要約"));
        let feed_cols: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('feeds') WHERE name='translate'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(feed_cols, 0);
        let art_cols: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name LIKE '%_ja'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(art_cols, 0);

        // idempotent on the new schema
        super::init(&conn).unwrap();
    }

    #[test]
    fn parses_plain_array() {
        let out = r#"[{"id": 1, "title_ja": "テスト", "summary_ja": null}]"#;
        let items = parse_translations(out).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title_ja, "テスト");
        assert!(items[0].summary_ja.is_none());
    }

    #[test]
    fn tolerates_surrounding_text_and_fences() {
        let out = "はい、翻訳します。\n```json\n[{\"id\": 2, \"title_ja\": \"タイトル\", \"summary_ja\": \"要約です。\\n\\n二段落目。\"}]\n```\n";
        let items = parse_translations(out).unwrap();
        assert_eq!(items[0].id, 2);
        assert!(items[0].summary_ja.as_deref().unwrap().contains("二段落目"));
    }

    #[test]
    fn missing_summary_field_defaults_to_none() {
        let out = r#"[{"id": 3, "title_ja": "タイトルのみ"}]"#;
        let items = parse_translations(out).unwrap();
        assert!(items[0].summary_ja.is_none());
    }
}
