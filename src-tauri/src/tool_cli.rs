//! Read-only command interface used by the embedded Pi agent.
//!
//! The running app exposes its own executable and database path through
//! environment variables. Pi can then query the local archive with its bash
//! tool without giving the model arbitrary SQL access.

use crate::{db, fetcher};
use serde::Serialize;
use std::path::Path;

const DEFAULT_LIMIT: i64 = 20;
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
            print_json(&articles.into_iter().map(article_result).collect::<Vec<_>>())
        }
        "recent" => {
            let unread_only = args.iter().any(|arg| arg == "--unread");
            let mut articles = db::list_articles(&conn, None, None, unread_only, false)
                .map_err(|e| e.to_string())?;
            articles.truncate(DEFAULT_LIMIT as usize);
            print_json(&articles.into_iter().map(article_result).collect::<Vec<_>>())
        }
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
                "myfocus local archive commands:\n  search <query>\n  recent [--unread]\n  article <id>\n  feeds\n  stats"
            );
            Ok(())
        }
        _ => Err(format!("不明なコマンド: {command}")),
    }
}

fn article_result(article: db::Article) -> ArticleResult {
    ArticleResult {
        id: article.id,
        feed: article.feed_title,
        title: article.title,
        url: article.url,
        published_at: article.published_at,
        read: article.read,
        starred: article.starred,
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
