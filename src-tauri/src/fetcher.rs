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
            let content_html = e
                .content
                .and_then(|c| c.body)
                .or_else(|| summary_html.clone());
            NewArticle {
                guid: e.id,
                title: e.title.map(|t| t.content).unwrap_or_default(),
                url: link,
                author: e.authors.first().map(|a| a.name.clone()),
                summary: summary_html.map(|s| strip_html(&s, 500)),
                content_html,
                published_at: e.published.or(e.updated).map(|d| d.timestamp()),
            }
        })
        .collect();

    Ok(FetchedFeed {
        title,
        site_url,
        articles,
    })
}

/// Fetch a web page and reduce it to readable text for LLM summarization.
pub async fn fetch_page_text(
    client: &reqwest::Client,
    url: &str,
    max_chars: usize,
) -> Result<String, String> {
    let res = client
        .get(url)
        .header("User-Agent", "myfocus/0.1 (+https://github.com/tanabe1478)")
        .send()
        .await
        .map_err(|e| format!("ページを取得できません: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("HTTP {} が返されました", res.status()));
    }
    let html = res.text().await.map_err(|e| e.to_string())?;
    let html = remove_blocks(&html, "script");
    let html = remove_blocks(&html, "style");
    let text = strip_html(&html, max_chars);
    if text.trim().is_empty() {
        return Err("本文テキストを抽出できませんでした".to_string());
    }
    Ok(text)
}

/// Remove `<tag ...>...</tag>` blocks (case-insensitive), e.g. script/style.
fn remove_blocks(html: &str, tag: &str) -> String {
    let lower = html.to_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    while let Some(start) = lower[pos..].find(&open).map(|i| pos + i) {
        out.push_str(&html[pos..start]);
        match lower[start..].find(&close) {
            Some(end) => pos = start + end + close.len(),
            None => return out, // unclosed block: drop the rest
        }
    }
    out.push_str(&html[pos..]);
    out
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
