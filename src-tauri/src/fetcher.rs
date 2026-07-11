use crate::db::NewArticle;
use feed_rs::parser;

pub struct FetchedFeed {
    pub title: String,
    pub site_url: Option<String>,
    pub articles: Vec<NewArticle>,
}

pub async fn fetch_feed(client: &reqwest::Client, url: &str) -> Result<FetchedFeed, String> {
    let res = client
        .get(url)
        .header("User-Agent", "myfocus/0.1 (+https://github.com/tanabe1478)")
        .send()
        .await
        .map_err(|e| format!("取得に失敗しました: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("HTTP {} が返されました", res.status()));
    }
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    let feed = parser::parse(&bytes[..]).map_err(|e| format!("フィードを解析できません: {e}"))?;

    let title = feed
        .title
        .map(|t| t.content)
        .unwrap_or_else(|| url.to_string());
    let site_url = feed
        .links
        .iter()
        .find(|l| l.rel.as_deref() != Some("self"))
        .map(|l| l.href.clone());

    let articles = feed
        .entries
        .into_iter()
        .map(|e| {
            let link = e.links.first().map(|l| l.href.clone());
            let summary_html = e.summary.map(|s| s.content);
            let content_html = e.content.and_then(|c| c.body).or_else(|| summary_html.clone());
            NewArticle {
                guid: e.id,
                title: e.title.map(|t| t.content).unwrap_or_default(),
                url: link,
                author: e.authors.first().map(|a| a.name.clone()),
                summary: summary_html.map(|s| strip_html(&s, 500)),
                content_html,
                published_at: e
                    .published
                    .or(e.updated)
                    .map(|d| d.timestamp()),
            }
        })
        .collect();

    Ok(FetchedFeed { title, site_url, articles })
}

/// Crude tag stripper for list summaries; full content is rendered (sanitized) in the frontend.
pub fn strip_html(html: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(html.len().min(max_chars * 4));
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let cleaned = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let trimmed: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(max_chars).collect()
}
