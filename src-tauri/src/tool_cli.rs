//! Read-only command interface used by the embedded Pi agent.
//!
//! The running app exposes its own executable and database path through
//! environment variables. Pi can then query the local archive with its bash
//! tool without giving the model arbitrary SQL access.

use crate::{db, fetcher};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

const DEFAULT_LIMIT: i64 = 20;
const RECOMMENDATION_CANDIDATE_LIMIT: usize = 50;
const RECOMMENDATION_HISTORY_LIMIT: usize = 20;
const MAX_ARTICLE_CHARS: usize = 15_000;

#[derive(Serialize)]
struct ArticleResult {
    id: i64,
    feed: String,
    title: String,
    url: Option<String>,
    published_at: Option<i64>,
    read: bool,
    starred: bool,
    feedback: Option<i8>,
    summary: Option<String>,
}

#[derive(Serialize)]
struct ArticleDetail {
    id: i64,
    feed: String,
    title: String,
    url: Option<String>,
    author: Option<String>,
    published_at: Option<i64>,
    read: bool,
    starred: bool,
    ai_summary: Option<String>,
    text: Option<String>,
}

#[derive(Serialize)]
struct Stats {
    feeds: i64,
    articles: i64,
    unread: i64,
    read: i64,
    starred: i64,
}

#[derive(Serialize)]
struct InterestArticle {
    id: i64,
    feed: String,
    title: String,
    published_at: Option<i64>,
}

#[derive(Serialize)]
struct FeedAffinity {
    feed: String,
    read_count: i64,
    starred_count: i64,
    liked_count: i64,
    disliked_count: i64,
}

#[derive(Serialize)]
struct RecommendationContext {
    candidates: Vec<ArticleResult>,
    liked: Vec<InterestArticle>,
    disliked: Vec<InterestArticle>,
    starred: Vec<InterestArticle>,
    recently_read: Vec<InterestArticle>,
    feed_affinity: Vec<FeedAffinity>,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let db_path = std::env::var("MYFOCUS_DB_PATH")
        .map_err(|_| "MYFOCUS_DB_PATHが設定されていません".to_string())?;
    let conn = db::open_read_only(Path::new(&db_path)).map_err(|e| e.to_string())?;
    let command = args.first().map(String::as_str).unwrap_or("help");

    match command {
        "search" => {
            let query = args[1..].join(" ");
            if query.trim().is_empty() {
                return Err("使い方: search <query>".to_string());
            }
            let articles =
                db::search_articles(&conn, &query, DEFAULT_LIMIT).map_err(|e| e.to_string())?;
            print_json(
                &articles
                    .into_iter()
                    .map(|article| article_result(article, None))
                    .collect::<Vec<_>>(),
            )
        }
        "recent" => {
            let unread_only = args.iter().any(|arg| arg == "--unread");
            let mut articles = db::list_articles(&conn, None, None, unread_only, false)
                .map_err(|e| e.to_string())?;
            articles.truncate(DEFAULT_LIMIT as usize);
            print_json(
                &articles
                    .into_iter()
                    .map(|article| article_result(article, None))
                    .collect::<Vec<_>>(),
            )
        }
        "recommend" => print_json(&recommendation_context(&conn)?),
        "article" => {
            let id = args
                .get(1)
                .ok_or("使い方: article <id>")?
                .parse::<i64>()
                .map_err(|_| "article idが不正です".to_string())?;
            let article = db::get_article(&conn, id).map_err(|e| e.to_string())?;
            let text = article
                .full_text
                .clone()
                .or_else(|| {
                    article
                        .content_html
                        .as_deref()
                        .map(|html| fetcher::strip_html(html, MAX_ARTICLE_CHARS))
                })
                .or_else(|| article.summary.clone())
                .map(|text| text.chars().take(MAX_ARTICLE_CHARS).collect());
            print_json(&ArticleDetail {
                id: article.id,
                feed: article.feed_title,
                title: article.title,
                url: article.url,
                author: article.author,
                published_at: article.published_at,
                read: article.read,
                starred: article.starred,
                ai_summary: article.ai_summary,
                text,
            })
        }
        "feeds" => {
            let feeds = db::list_feeds(&conn).map_err(|e| e.to_string())?;
            print_json(&feeds)
        }
        "stats" => {
            let stats = Stats {
                feeds: conn
                    .query_row("SELECT COUNT(*) FROM feeds", [], |row| row.get(0))
                    .map_err(|e| e.to_string())?,
                articles: conn
                    .query_row("SELECT COUNT(*) FROM articles", [], |row| row.get(0))
                    .map_err(|e| e.to_string())?,
                unread: conn
                    .query_row("SELECT COUNT(*) FROM articles WHERE read = 0", [], |row| {
                        row.get(0)
                    })
                    .map_err(|e| e.to_string())?,
                read: conn
                    .query_row("SELECT COUNT(*) FROM articles WHERE read = 1", [], |row| {
                        row.get(0)
                    })
                    .map_err(|e| e.to_string())?,
                starred: conn
                    .query_row(
                        "SELECT COUNT(*) FROM articles WHERE starred = 1",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?,
            };
            print_json(&stats)
        }
        "help" | "--help" | "-h" => {
            println!(
                "myfocus local archive commands:\n  search <query>\n  recent [--unread]\n  recommend\n  article <id>\n  feeds\n  stats"
            );
            Ok(())
        }
        _ => Err(format!("不明なコマンド: {command}")),
    }
}

fn recommendation_context(conn: &Connection) -> Result<RecommendationContext, String> {
    let feedback: HashMap<i64, i8> = db::list_ai_feedback(conn)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let candidates = db::list_articles(conn, None, None, true, false)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|article| feedback.get(&article.id) != Some(&-1))
        .take(RECOMMENDATION_CANDIDATE_LIMIT)
        .map(|article| {
            let value = feedback.get(&article.id).copied();
            article_result(article, value)
        })
        .collect();

    let liked = feedback_articles(conn, 1)?;
    let disliked = feedback_articles(conn, -1)?;

    let mut starred = conn
        .prepare(
            "SELECT a.id, f.title, a.title, a.published_at
             FROM articles a JOIN feeds f ON f.id = a.feed_id
             WHERE a.starred = 1
             ORDER BY COALESCE(a.published_at, 0) DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let starred = starred
        .query_map([RECOMMENDATION_HISTORY_LIMIT as i64], |row| {
            Ok(InterestArticle {
                id: row.get(0)?,
                feed: row.get(1)?,
                title: row.get(2)?,
                published_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut recently_read = conn
        .prepare(
            "SELECT a.id, f.title, a.title, a.published_at
             FROM articles a JOIN feeds f ON f.id = a.feed_id
             WHERE a.read = 1
             ORDER BY COALESCE(a.published_at, 0) DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let recently_read = recently_read
        .query_map([RECOMMENDATION_HISTORY_LIMIT as i64], |row| {
            Ok(InterestArticle {
                id: row.get(0)?,
                feed: row.get(1)?,
                title: row.get(2)?,
                published_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut affinity = conn
        .prepare(
            "SELECT f.title,
                    SUM(CASE WHEN a.read = 1 THEN 1 ELSE 0 END) AS read_count,
                    SUM(CASE WHEN a.starred = 1 THEN 1 ELSE 0 END) AS starred_count,
                    SUM(CASE WHEN af.value = 1 THEN 1 ELSE 0 END) AS liked_count,
                    SUM(CASE WHEN af.value = -1 THEN 1 ELSE 0 END) AS disliked_count
             FROM articles a
             JOIN feeds f ON f.id = a.feed_id
             LEFT JOIN article_ai_feedback af ON af.article_id = a.id
             WHERE a.read = 1 OR a.starred = 1 OR af.value IS NOT NULL
             GROUP BY f.id, f.title
             ORDER BY liked_count DESC, starred_count DESC, read_count DESC
             LIMIT 15",
        )
        .map_err(|e| e.to_string())?;
    let feed_affinity = affinity
        .query_map([], |row| {
            Ok(FeedAffinity {
                feed: row.get(0)?,
                read_count: row.get(1)?,
                starred_count: row.get(2)?,
                liked_count: row.get(3)?,
                disliked_count: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(RecommendationContext {
        candidates,
        liked,
        disliked,
        starred,
        recently_read,
        feed_affinity,
    })
}

fn feedback_articles(conn: &Connection, value: i8) -> Result<Vec<InterestArticle>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT a.id, f.title, a.title, a.published_at
             FROM article_ai_feedback af
             JOIN articles a ON a.id = af.article_id
             JOIN feeds f ON f.id = a.feed_id
             WHERE af.value = ?1
             ORDER BY af.updated_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![value, RECOMMENDATION_HISTORY_LIMIT as i64],
            |row| {
                Ok(InterestArticle {
                    id: row.get(0)?,
                    feed: row.get(1)?,
                    title: row.get(2)?,
                    published_at: row.get(3)?,
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    rows
}

fn article_result(article: db::Article, feedback: Option<i8>) -> ArticleResult {
    ArticleResult {
        id: article.id,
        feed: article.feed_title,
        title: article.title,
        url: article.url,
        published_at: article.published_at,
        read: article.read,
        starred: article.starred,
        feedback,
        summary: article.summary,
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_context_contains_candidates_and_interest_signals() {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("recommend.db")).unwrap();
        conn.execute(
            "INSERT INTO feeds (id, url, title) VALUES (1, 'https://example.com/feed', 'Example')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles
             (id, feed_id, guid, title, summary, published_at, read, starred)
             VALUES
             (1, 1, 'unread', 'Unread candidate', 'candidate summary', 300, 0, 0),
             (2, 1, 'starred', 'Starred interest', NULL, 200, 1, 1),
             (3, 1, 'read', 'Recent interest', NULL, 100, 1, 0)",
            [],
        )
        .unwrap();
        db::set_ai_feedback(&conn, 1, 1).unwrap();
        db::set_ai_feedback(&conn, 3, -1).unwrap();

        let context = recommendation_context(&conn).unwrap();
        assert_eq!(context.candidates.len(), 1);
        assert_eq!(context.candidates[0].id, 1);
        assert_eq!(context.candidates[0].feedback, Some(1));
        assert_eq!(context.liked[0].id, 1);
        assert_eq!(context.disliked[0].id, 3);
        assert_eq!(context.starred[0].id, 2);
        assert_eq!(context.recently_read[0].id, 2);
        assert_eq!(context.feed_affinity[0].feed, "Example");
        assert_eq!(context.feed_affinity[0].read_count, 2);
        assert_eq!(context.feed_affinity[0].starred_count, 1);
        assert_eq!(context.feed_affinity[0].liked_count, 1);
        assert_eq!(context.feed_affinity[0].disliked_count, 1);
    }
}
