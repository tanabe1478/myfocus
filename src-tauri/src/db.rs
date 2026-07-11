use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone)]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub site_url: Option<String>,
    pub last_fetched_at: Option<i64>,
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
    pub published_at: Option<i64>,
    pub read: bool,
    pub starred: bool,
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
            published_at INTEGER,
            read INTEGER NOT NULL DEFAULT 0,
            starred INTEGER NOT NULL DEFAULT 0,
            UNIQUE(feed_id, guid)
        );

        CREATE INDEX IF NOT EXISTS idx_articles_feed ON articles(feed_id, published_at DESC);
        CREATE INDEX IF NOT EXISTS idx_articles_read ON articles(read, published_at DESC);
        "#,
    )?;
    Ok(conn)
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
        published_at: row.get(9)?,
        read: row.get::<_, i64>(10)? != 0,
        starred: row.get::<_, i64>(11)? != 0,
    })
}

const ARTICLE_COLS: &str = "a.id, a.feed_id, f.title, a.guid, a.title, a.url, a.author, a.summary, a.content_html, a.published_at, a.read, a.starred";

pub fn list_feeds(conn: &Connection) -> rusqlite::Result<Vec<Feed>> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.url, f.title, f.site_url, f.last_fetched_at,
                (SELECT COUNT(*) FROM articles a WHERE a.feed_id = f.id AND a.read = 0)
         FROM feeds f ORDER BY f.title COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Feed {
            id: row.get(0)?,
            url: row.get(1)?,
            title: row.get(2)?,
            site_url: row.get(3)?,
            last_fetched_at: row.get(4)?,
            unread_count: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn upsert_feed(conn: &Connection, url: &str, title: &str, site_url: Option<&str>) -> rusqlite::Result<i64> {
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
pub fn insert_articles(conn: &mut Connection, feed_id: i64, articles: &[NewArticle]) -> rusqlite::Result<usize> {
    let tx = conn.transaction()?;
    let mut inserted = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO articles (feed_id, guid, title, url, author, summary, content_html, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for a in articles {
            inserted += stmt.execute(params![
                feed_id, a.guid, a.title, a.url, a.author, a.summary, a.content_html, a.published_at
            ])?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

pub fn list_articles(
    conn: &Connection,
    feed_id: Option<i64>,
    unread_only: bool,
    starred_only: bool,
    limit: i64,
) -> rusqlite::Result<Vec<Article>> {
    let mut sql = format!(
        "SELECT {ARTICLE_COLS} FROM articles a JOIN feeds f ON f.id = a.feed_id WHERE 1=1"
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(fid) = feed_id {
        sql.push_str(" AND a.feed_id = ?");
        p.push(Box::new(fid));
    }
    if unread_only {
        sql.push_str(" AND a.read = 0");
    }
    if starred_only {
        sql.push_str(" AND a.starred = 1");
    }
    sql.push_str(" ORDER BY a.published_at DESC, a.id DESC LIMIT ?");
    p.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(p.iter().map(|b| b.as_ref())), row_to_article)?;
    rows.collect()
}

/// All articles' (id, searchable text) for the fuzzy index.
pub fn search_corpus(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.title || ' ' || COALESCE(a.summary, '') || ' ' || f.title
         FROM articles a JOIN feeds f ON f.id = a.feed_id
         ORDER BY a.published_at DESC LIMIT 20000",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn articles_by_ids(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<Article>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT {ARTICLE_COLS} FROM articles a JOIN feeds f ON f.id = a.feed_id WHERE a.id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), row_to_article)?;
    let mut found: Vec<Article> = rows.collect::<Result<_, _>>()?;
    // preserve ranking order given by ids
    found.sort_by_key(|a| ids.iter().position(|&id| id == a.id).unwrap_or(usize::MAX));
    Ok(found)
}

pub fn set_read(conn: &Connection, article_id: i64, read: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE articles SET read = ?1 WHERE id = ?2", params![read as i64, article_id])?;
    Ok(())
}

pub fn set_starred(conn: &Connection, article_id: i64, starred: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE articles SET starred = ?1 WHERE id = ?2", params![starred as i64, article_id])?;
    Ok(())
}

pub fn mark_all_read(conn: &Connection, feed_id: Option<i64>) -> rusqlite::Result<()> {
    match feed_id {
        Some(fid) => conn.execute("UPDATE articles SET read = 1 WHERE feed_id = ?1", [fid])?,
        None => conn.execute("UPDATE articles SET read = 1", [])?,
    };
    Ok(())
}

pub fn remove_feed(conn: &Connection, feed_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM feeds WHERE id = ?1", [feed_id])?;
    Ok(())
}

/// Register a feed without fetching it yet (OPML import). Keeps existing rows untouched.
pub fn insert_feed_stub(conn: &Connection, url: &str, title: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO feeds (url, title) VALUES (?1, ?2)",
        params![url, title],
    )?;
    Ok(n > 0)
}

pub fn feed_urls(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, url FROM feeds")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}
