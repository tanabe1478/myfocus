use crate::{db, fetcher};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Only one translation run at a time; refresh cycles just skip if one is active.
static TRANSLATING: AtomicBool = AtomicBool::new(false);

// bodies make prompts heavy, so batches stay small
const BATCH_SIZE: i64 = 4;
// safety cap per run: a large backlog continues on the next refresh cycle
const MAX_BATCHES: usize = 30;
const BODY_MAX_CHARS: usize = 6000;
const PI_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
struct TranslatedItem {
    id: i64,
    title_ja: String,
    #[serde(default)]
    summary_ja: Option<String>,
}

#[derive(serde::Serialize, Clone)]
struct TranslateStatus {
    active: bool,
    remaining: i64,
}

fn emit_status(app: &AppHandle, active: bool) {
    let remaining = (|| -> Result<i64, String> {
        let state = app.state::<crate::AppState>();
        let conn = crate::lock_db(&state)?;
        db::count_untranslated(&conn).map_err(|e| e.to_string())
    })()
    .unwrap_or(0);
    let _ = app.emit("translate-status", TranslateStatus { active, remaining });
}

/// Start translating pending articles in the background (no-op if already running).
pub fn kick(app: &AppHandle) {
    if TRANSLATING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run(&app).await;
        TRANSLATING.store(false, Ordering::SeqCst);
        emit_status(&app, false);
        if let Err(e) = result {
            let _ = app.emit("translate-error", &e);
        }
    });
}

async fn run(app: &AppHandle) -> Result<(), String> {
    for _ in 0..MAX_BATCHES {
        let (batch, model, client) = {
            let state = app.state::<crate::AppState>();
            let conn = crate::lock_db(&state)?;
            let batch = db::untranslated_articles(&conn, BATCH_SIZE).map_err(|e| e.to_string())?;
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
                db::store_translation(&conn, t.id, t.title_ja.trim(), summary)
                    .map_err(|e| e.to_string())?;
            }
            // items the model skipped would be re-requested forever; mark them
            // as "translated" with the original title so the queue drains
            let returned: std::collections::HashSet<i64> = translated.iter().map(|t| t.id).collect();
            for item in &batch {
                if !returned.contains(&item.id) {
                    let _ = db::store_translation(&conn, item.id, &item.title, None);
                }
            }
        }
        let _ = app.emit("feeds-updated", ());
    }
    Ok(())
}

async fn digest_batch(
    batch: &[db::TranslateItem],
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
        r#"あなたはRSSリーダーの翻訳・要約エンジンです。以下のJSON配列の各記事について:

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

/// Run `pi -p` with all agentic features disabled and return its text output.
/// Used for stateless jobs (translation, summarization) so they don't touch
/// the chat assistant's RPC session.
pub async fn pi_print(prompt: &str, model: Option<&str>) -> Result<String, String> {
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
