use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone)]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub site_url: Option<String>,
    pub category: Option<String>,
    pub last_fetched_at: Option<i64>,
    pub last_error: Option<String>,
    pub unread_count: i64,
}

#[derive(Serialize, Clone)]
pub struct Article {
    pub id: i64,
    pub feed_id: i64,
    pub feed_title: String,
    pub guid: String,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub content_html: Option<String>,
    pub full_text: Option<String>,
    pub ai_summary: Option<String>,
    pub ai_summary_model: Option<String>,
    pub ai_summary_status: Option<String>,
    pub ai_summary_error: Option<String>,
    pub ai_summary_reviewed: bool,
    pub published_at: Option<i64>,
    pub read: bool,
    pub starred: bool,
}

pub fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS feeds (
            id INTEGER PRIMARY KEY,
            url TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL DEFAULT '',
            site_url TEXT,
            last_fetched_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS articles (
            id INTEGER PRIMARY KEY,
            feed_id INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
            guid TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            url TEXT,
            author TEXT,
            summary TEXT,
            content_html TEXT,
            full_text TEXT,
            ai_summary TEXT,
            ai_summary_model TEXT,
            ai_summary_updated_at INTEGER,
            published_at INTEGER,
            read INTEGER NOT NULL DEFAULT 0,
            starred INTEGER NOT NULL DEFAULT 0,
            UNIQUE(feed_id, guid)
        );

        -- Lightweight tombstones prevent old read articles from returning as
        -- unread when a feed still advertises them after retention cleanup.
        CREATE TABLE IF NOT EXISTS article_read_history (
            feed_id INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
            guid TEXT NOT NULL,
            read_at INTEGER NOT NULL,
            PRIMARY KEY (feed_id, guid)
        );

        CREATE TABLE IF NOT EXISTS article_ai_feedback (
            article_id INTEGER PRIMARY KEY REFERENCES articles(id) ON DELETE CASCADE,
            value INTEGER NOT NULL CHECK(value IN (-1, 1)),
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS article_summary_jobs (
            article_id INTEGER PRIMARY KEY REFERENCES articles(id) ON DELETE CASCADE,
            status TEXT NOT NULL CHECK(status IN ('queued', 'running', 'completed', 'failed')),
            force INTEGER NOT NULL DEFAULT 0,
            error TEXT,
            requested_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            reviewed_at INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_summary_jobs_status
            ON article_summary_jobs(status, updated_at DESC);

        CREATE INDEX IF NOT EXISTS idx_articles_feed ON articles(feed_id, published_at DESC);
        CREATE INDEX IF NOT EXISTS idx_articles_read ON articles(read, published_at DESC);
        "#,
    )?;
    for (column, ddl) in [
        ("category", "ALTER TABLE feeds ADD COLUMN category TEXT"),
        ("last_error", "ALTER TABLE feeds ADD COLUMN last_error TEXT"),
    ] {
        let exists = conn
            .prepare("SELECT 1 FROM pragma_table_info('feeds') WHERE name = ?1")?
            .exists([column])?;
        if !exists {
            conn.execute(ddl, [])?;
        }
    }
    for (column, ddl) in [
        (
            "full_text",
            "ALTER TABLE articles ADD COLUMN full_text TEXT",
        ),
        (
            "ai_summary",
            "ALTER TABLE articles ADD COLUMN ai_summary TEXT",
        ),
        (
            "ai_summary_model",
            "ALTER TABLE articles ADD COLUMN ai_summary_model TEXT",
        ),
        (
            "ai_summary_updated_at",
            "ALTER TABLE articles ADD COLUMN ai_summary_updated_at INTEGER",
        ),
    ] {
        let exists = conn
            .prepare("SELECT 1 FROM pragma_table_info('articles') WHERE name = ?1")?
            .exists([column])?;
        if !exists {
            conn.execute(ddl, [])?;
        }
    }

    // FTS5 virtual tables cannot add columns. Recreate the old three-column
    // index once so extracted article bodies become searchable.
    let fts_exists = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'articles_fts'")?
        .exists([])?;
    let fts_has_full_text = fts_exists
        && conn
            .prepare("SELECT 1 FROM pragma_table_info('articles_fts') WHERE name = 'full_text'")?
            .exists([])?;
    if fts_exists && !fts_has_full_text {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS articles_fts_insert;
             DROP TRIGGER IF EXISTS articles_fts_delete;
             DROP TRIGGER IF EXISTS articles_fts_update;
             DROP TABLE articles_fts;",
        )?;
    }

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        -- full-text index over article metadata and extracted page body;
        -- trigram tokenization supports substring matching and Japanese text.
        CREATE VIRTUAL TABLE IF NOT EXISTS articles_fts USING fts5(
            title, summary, feed_title, full_text,
            tokenize = 'trigram'
        );

        CREATE TRIGGER IF NOT EXISTS articles_fts_insert AFTER INSERT ON articles BEGIN
            INSERT INTO articles_fts (rowid, title, summary, feed_title, full_text)
            VALUES (
                new.id, new.title, COALESCE(new.summary, ''),
                COALESCE((SELECT title FROM feeds WHERE id = new.feed_id), ''),
                COALESCE(new.full_text, '')
            );
        END;

        CREATE TRIGGER IF NOT EXISTS articles_fts_delete AFTER DELETE ON articles BEGIN
            DELETE FROM articles_fts WHERE rowid = old.id;
        END;

        CREATE TRIGGER IF NOT EXISTS articles_fts_update
        AFTER UPDATE OF title, summary, full_text ON articles BEGIN
            DELETE FROM articles_fts WHERE rowid = old.id;
            INSERT INTO articles_fts (rowid, title, summary, feed_title, full_text)
            VALUES (
                new.id, new.title, COALESCE(new.summary, ''),
                COALESCE((SELECT title FROM feeds WHERE id = new.feed_id), ''),
                COALESCE(new.full_text, '')
            );
        END;
        "#,
    )?;

    // Existing summaries remain available in history without appearing as new.
    conn.execute(
        "INSERT OR IGNORE INTO article_summary_jobs
           (article_id, status, force, requested_at, updated_at, reviewed_at)
         SELECT id, 'completed', 0,
                COALESCE(ai_summary_updated_at, strftime('%s','now')),
                COALESCE(ai_summary_updated_at, strftime('%s','now')),
                COALESCE(ai_summary_updated_at, strftime('%s','now'))
         FROM articles WHERE ai_summary IS NOT NULL",
        [],
    )?;
    // Jobs interrupted by an application restart are picked up again.
    conn.execute(
        "UPDATE article_summary_jobs SET status = 'queued', updated_at = strftime('%s','now')
         WHERE status = 'running'",
        [],
    )?;

    // Preserve the state of articles that were read before tombstones existed.
    conn.execute(
        "INSERT OR IGNORE INTO article_read_history (feed_id, guid, read_at)
         SELECT feed_id, guid, strftime('%s','now') FROM articles WHERE read = 1",
        [],
    )?;

    // backfill the FTS index for articles inserted before it existed
    conn.execute(
        "INSERT INTO articles_fts (rowid, title, summary, feed_title, full_text)
         SELECT a.id, a.title, COALESCE(a.summary, ''), f.title,
                COALESCE(a.full_text, '')
         FROM articles a JOIN feeds f ON f.id = a.feed_id
         WHERE a.id NOT IN (SELECT rowid FROM articles_fts)",
        [],
    )?;
    Ok(conn)
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        e => Err(e),
    })
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn list_ai_feedback(conn: &Connection) -> rusqlite::Result<Vec<(i64, i8)>> {
    let mut stmt = conn.prepare("SELECT article_id, value FROM article_ai_feedback")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    rows
}

pub fn set_ai_feedback(conn: &Connection, article_id: i64, value: i8) -> rusqlite::Result<()> {
    if value == 0 {
        conn.execute(
            "DELETE FROM article_ai_feedback WHERE article_id = ?1",
            [article_id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO article_ai_feedback (article_id, value, updated_at)
             VALUES (?1, ?2, strftime('%s','now'))
             ON CONFLICT(article_id) DO UPDATE SET
               value = excluded.value, updated_at = excluded.updated_at",
            params![article_id, value],
        )?;
    }
    Ok(())
}

/// Delete read, unstarred, non-summarized articles older than the configured age.
/// Articles without a publish date and AI summary history entries are kept.
pub fn cleanup_old_articles(conn: &Connection, days: i64) -> rusqlite::Result<usize> {
    // Keep only feed_id + guid before dropping the heavy article body. A later
    // feed refresh can then ignore the item instead of resurrecting it unread.
    conn.execute(
        "INSERT OR IGNORE INTO article_read_history (feed_id, guid, read_at)
         SELECT feed_id, guid, strftime('%s','now') FROM articles
         WHERE read = 1 AND starred = 0 AND ai_summary IS NULL
           AND published_at IS NOT NULL
           AND published_at < strftime('%s', 'now') - ?1 * 86400",
        [days],
    )?;
    conn.execute(
        "DELETE FROM articles
         WHERE read = 1 AND starred = 0 AND ai_summary IS NULL
           AND published_at IS NOT NULL
           AND published_at < strftime('%s', 'now') - ?1 * 86400",
        [days],
    )
}

pub fn set_feed_error(
    conn: &Connection,
    feed_id: i64,
    error: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE feeds SET last_error = ?1 WHERE id = ?2",
        params![error, feed_id],
    )?;
    Ok(())
}

fn row_to_article(row: &rusqlite::Row) -> rusqlite::Result<Article> {
    Ok(Article {
        id: row.get(0)?,
        feed_id: row.get(1)?,
        feed_title: row.get(2)?,
        guid: row.get(3)?,
        title: row.get(4)?,
        url: row.get(5)?,
        author: row.get(6)?,
        summary: row.get(7)?,
        content_html: row.get(8)?,
        full_text: row.get(9)?,
        ai_summary: row.get(10)?,
        ai_summary_model: row.get(11)?,
        ai_summary_status: row.get(12)?,
        ai_summary_error: row.get(13)?,
        ai_summary_reviewed: row.get::<_, i64>(14)? != 0,
        published_at: row.get(15)?,
        read: row.get::<_, i64>(16)? != 0,
        starred: row.get::<_, i64>(17)? != 0,
    })
}

const ARTICLE_COLS: &str = "a.id, a.feed_id, f.title, a.guid, a.title, a.url, a.author, a.summary, a.content_html, a.full_text, a.ai_summary, a.ai_summary_model, (SELECT status FROM article_summary_jobs j WHERE j.article_id = a.id), (SELECT error FROM article_summary_jobs j WHERE j.article_id = a.id), COALESCE((SELECT reviewed_at IS NOT NULL FROM article_summary_jobs j WHERE j.article_id = a.id), 0), a.published_at, a.read, a.starred";

// list views skip content_html (it can be tens of KB per article); the reading
// pane loads the full row via get_article
const ARTICLE_LIST_COLS: &str = "a.id, a.feed_id, f.title, a.guid, a.title, a.url, a.author, a.summary, NULL, NULL, a.ai_summary, a.ai_summary_model, (SELECT status FROM article_summary_jobs j WHERE j.article_id = a.id), (SELECT error FROM article_summary_jobs j WHERE j.article_id = a.id), COALESCE((SELECT reviewed_at IS NOT NULL FROM article_summary_jobs j WHERE j.article_id = a.id), 0), a.published_at, a.read, a.starred";

pub fn list_feeds(conn: &Connection) -> rusqlite::Result<Vec<Feed>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.url, f.title, f.site_url, f.category, f.last_fetched_at, f.last_error,
                (SELECT COUNT(*) FROM articles a WHERE a.feed_id = f.id AND a.read = 0)
         FROM feeds f ORDER BY f.category COLLATE NOCASE, f.title COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Feed {
            id: row.get(0)?,
            url: row.get(1)?,
            title: row.get(2)?,
            site_url: row.get(3)?,
            category: row.get(4)?,
            last_fetched_at: row.get(5)?,
            last_error: row.get(6)?,
            unread_count: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn upsert_feed(
    conn: &Connection,
    url: &str,
    title: &str,
    site_url: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO feeds (url, title, site_url, last_fetched_at) VALUES (?1, ?2, ?3, strftime('%s','now'))
         ON CONFLICT(url) DO UPDATE SET title = excluded.title, site_url = excluded.site_url,
         last_fetched_at = excluded.last_fetched_at",
        params![url, title, site_url],
    )?;
    let id: i64 = conn.query_row("SELECT id FROM feeds WHERE url = ?1", [url], |r| r.get(0))?;
    Ok(id)
}

pub struct NewArticle {
    pub guid: String,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub content_html: Option<String>,
    pub published_at: Option<i64>,
}

/// Insert articles, ignoring ones already stored. Returns number of new rows.
pub fn insert_articles(
    conn: &mut Connection,
    feed_id: i64,
    articles: &[NewArticle],
) -> rusqlite::Result<usize> {
    let tx = conn.transaction()?;
    let mut inserted = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO articles (feed_id, guid, title, url, author, summary, content_html, published_at)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
             WHERE NOT EXISTS (
                 SELECT 1 FROM article_read_history h
                 WHERE h.feed_id = ?1 AND h.guid = ?2
             )",
        )?;
        for a in articles {
            inserted += stmt.execute(params![
                feed_id,
                a.guid,
                a.title,
                a.url,
                a.author,
                a.summary,
                a.content_html,
                a.published_at
            ])?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

pub fn list_articles(
    conn: &Connection,
    feed_id: Option<i64>,
    category: Option<&str>,
    unread_only: bool,
    starred_only: bool,
    summarized_only: bool,
) -> rusqlite::Result<Vec<Article>> {
    let mut sql = format!(
        "SELECT {ARTICLE_LIST_COLS} FROM articles a JOIN feeds f ON f.id = a.feed_id WHERE 1=1"
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(fid) = feed_id {
        sql.push_str(" AND a.feed_id = ?");
        p.push(Box::new(fid));
    }
    if let Some(cat) = category {
        sql.push_str(" AND f.category = ?");
        p.push(Box::new(cat.to_string()));
    }
    if unread_only {
        sql.push_str(" AND a.read = 0");
    }
    if starred_only {
        sql.push_str(" AND a.starred = 1");
    }
    if summarized_only {
        sql.push_str(" AND (a.ai_summary IS NOT NULL OR EXISTS (SELECT 1 FROM article_summary_jobs j WHERE j.article_id = a.id))");
        sql.push_str(" ORDER BY COALESCE((SELECT updated_at FROM article_summary_jobs j WHERE j.article_id = a.id), a.ai_summary_updated_at) DESC, a.id DESC");
    } else {
        sql.push_str(" ORDER BY a.published_at DESC, a.id DESC");
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(p.iter().map(|b| b.as_ref())),
        row_to_article,
    )?;
    rows.collect()
}

pub fn get_article(conn: &Connection, article_id: i64) -> rusqlite::Result<Article> {
    let sql = format!(
        "SELECT {ARTICLE_COLS} FROM articles a JOIN feeds f ON f.id = a.feed_id WHERE a.id = ?1"
    );
    conn.query_row(&sql, [article_id], row_to_article)
}

pub fn store_full_text(
    conn: &Connection,
    article_id: i64,
    full_text: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE articles SET full_text = ?1 WHERE id = ?2",
        params![full_text, article_id],
    )?;
    Ok(())
}

pub fn store_ai_summary(
    conn: &Connection,
    article_id: i64,
    summary: &str,
    model: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE articles
         SET ai_summary = ?1, ai_summary_model = ?2,
             ai_summary_updated_at = strftime('%s','now')
         WHERE id = ?3",
        params![summary, model, article_id],
    )?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryStats {
    pub pending: i64,
    pub unreviewed: i64,
    pub failed: i64,
}

pub fn queue_summary_job(
    conn: &Connection,
    article_id: i64,
    force: bool,
) -> rusqlite::Result<bool> {
    let active = conn
        .prepare(
            "SELECT 1 FROM article_summary_jobs
             WHERE article_id = ?1 AND status IN ('queued', 'running')",
        )?
        .exists([article_id])?;
    if active {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO article_summary_jobs
           (article_id, status, force, error, requested_at, updated_at, reviewed_at)
         VALUES (?1, 'queued', ?2, NULL, strftime('%s','now'), strftime('%s','now'), NULL)
         ON CONFLICT(article_id) DO UPDATE SET
           status = 'queued', force = excluded.force, error = NULL,
           requested_at = excluded.requested_at, updated_at = excluded.updated_at,
           reviewed_at = NULL",
        params![article_id, force as i64],
    )?;
    Ok(true)
}

pub fn queued_summary_jobs(conn: &Connection) -> rusqlite::Result<Vec<(i64, bool)>> {
    let mut stmt = conn.prepare(
        "SELECT article_id, force FROM article_summary_jobs
         WHERE status = 'queued' ORDER BY requested_at",
    )?;
    let jobs = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)))?
        .collect();
    jobs
}

pub fn start_summary_job(conn: &Connection, article_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE article_summary_jobs
         SET status = 'running', updated_at = strftime('%s','now'), error = NULL
         WHERE article_id = ?1",
        [article_id],
    )?;
    Ok(())
}

pub fn complete_summary_job(conn: &Connection, article_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE article_summary_jobs
         SET status = 'completed', updated_at = strftime('%s','now'), error = NULL,
             reviewed_at = NULL
         WHERE article_id = ?1",
        [article_id],
    )?;
    Ok(())
}

pub fn fail_summary_job(conn: &Connection, article_id: i64, error: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE article_summary_jobs
         SET status = 'failed', updated_at = strftime('%s','now'), error = ?2
         WHERE article_id = ?1",
        params![article_id, error.chars().take(1000).collect::<String>()],
    )?;
    Ok(())
}

pub fn mark_summary_reviewed(conn: &Connection, article_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE article_summary_jobs SET reviewed_at = strftime('%s','now')
         WHERE article_id = ?1 AND status = 'completed'",
        [article_id],
    )?;
    Ok(())
}

pub fn summary_stats(conn: &Connection) -> rusqlite::Result<SummaryStats> {
    conn.query_row(
        "SELECT
           SUM(CASE WHEN status IN ('queued', 'running') THEN 1 ELSE 0 END),
           SUM(CASE WHEN status = 'completed' AND reviewed_at IS NULL THEN 1 ELSE 0 END),
           SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END)
         FROM article_summary_jobs",
        [],
        |row| {
            Ok(SummaryStats {
                pending: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                unreviewed: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                failed: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            })
        },
    )
}

/// Full-text search over title, summary, feed title, and extracted page body.
/// The trigram tokenizer needs 3+ character terms, so short queries (common in
/// Japanese) fall back to plain substring matching.
pub fn search_articles(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<Article>> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    if terms.iter().all(|t| t.chars().count() >= 3) {
        // quote each term so user input is treated literally, AND them together
        let match_expr = terms
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "SELECT {ARTICLE_LIST_COLS}
             FROM articles_fts fts
             JOIN articles a ON a.id = fts.rowid
             JOIN feeds f ON f.id = a.feed_id
             WHERE articles_fts MATCH ?1
             ORDER BY bm25(articles_fts), a.published_at DESC LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![match_expr, limit], row_to_article)?;
        return rows.collect();
    }

    let mut sql = format!(
        "SELECT {ARTICLE_LIST_COLS} FROM articles a JOIN feeds f ON f.id = a.feed_id WHERE 1=1"
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    for t in &terms {
        sql.push_str(
            " AND (a.title LIKE ?1 ESCAPE '\\' OR COALESCE(a.summary,'') LIKE ?1 ESCAPE '\\' OR f.title LIKE ?1 ESCAPE '\\' OR COALESCE(a.full_text,'') LIKE ?1 ESCAPE '\\')"
                .replace("?1", &format!("?{}", p.len() + 1))
                .as_str(),
        );
        let escaped = t
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        p.push(Box::new(format!("%{escaped}%")));
    }
    sql.push_str(&format!(
        " ORDER BY a.published_at DESC LIMIT ?{}",
        p.len() + 1
    ));
    p.push(Box::new(limit));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(p.iter().map(|b| b.as_ref())),
        row_to_article,
    )?;
    rows.collect()
}

pub fn set_read(conn: &Connection, article_id: i64, read: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE articles SET read = ?1 WHERE id = ?2",
        params![read as i64, article_id],
    )?;
    if read {
        conn.execute(
            "INSERT OR REPLACE INTO article_read_history (feed_id, guid, read_at)
             SELECT feed_id, guid, strftime('%s','now') FROM articles WHERE id = ?1",
            [article_id],
        )?;
    } else {
        conn.execute(
            "DELETE FROM article_read_history
             WHERE (feed_id, guid) = (SELECT feed_id, guid FROM articles WHERE id = ?1)",
            [article_id],
        )?;
    }
    Ok(())
}

pub fn set_starred(conn: &Connection, article_id: i64, starred: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE articles SET starred = ?1 WHERE id = ?2",
        params![starred as i64, article_id],
    )?;
    Ok(())
}

pub fn mark_all_read(
    conn: &Connection,
    feed_id: Option<i64>,
    category: Option<&str>,
) -> rusqlite::Result<()> {
    match (feed_id, category) {
        (Some(fid), _) => conn.execute("UPDATE articles SET read = 1 WHERE feed_id = ?1", [fid])?,
        (None, Some(cat)) => conn.execute(
            "UPDATE articles SET read = 1
             WHERE feed_id IN (SELECT id FROM feeds WHERE category = ?1)",
            [cat],
        )?,
        (None, None) => conn.execute("UPDATE articles SET read = 1", [])?,
    };
    conn.execute(
        "INSERT OR REPLACE INTO article_read_history (feed_id, guid, read_at)
         SELECT feed_id, guid, strftime('%s','now') FROM articles WHERE read = 1",
        [],
    )?;
    Ok(())
}

pub fn remove_feed(conn: &Connection, feed_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM feeds WHERE id = ?1", [feed_id])?;
    Ok(())
}

/// Register a feed without fetching it yet (OPML import).
/// New feeds get the given category; existing ones have their category updated.
pub fn insert_feed_stub(
    conn: &Connection,
    url: &str,
    title: &str,
    category: Option<&str>,
) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO feeds (url, title, category) VALUES (?1, ?2, ?3)",
        params![url, title, category],
    )?;
    if n == 0 {
        conn.execute(
            "UPDATE feeds SET category = ?1 WHERE url = ?2",
            params![category, url],
        )?;
    }
    Ok(n > 0)
}

pub fn feed_urls(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, url FROM feeds")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}
