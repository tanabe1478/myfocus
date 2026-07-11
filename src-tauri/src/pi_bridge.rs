use serde_json::json;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

/// Handle to a running `pi --mode rpc` subprocess.
pub struct PiBridge {
    inner: Mutex<Option<PiProc>>,
}

struct PiProc {
    child: Child,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
}

const SYSTEM_PROMPT: &str = r#"あなたはRSSリーダー「myfocus」に組み込まれたアシスタントです。役割:
1. ユーザーが読んでいる記事についての相談・要約・解説に日本語で答える。
2. 「記事を探して」「〜についてのフィードを探して」と頼まれたら、bashツールのcurlなどでWeb検索・取得を行い、記事のタイトルとURL、可能ならRSS/AtomフィードのURLを提示する。
3. フィードURLを提案するときは必ず `FEED: <url>` という行を含める（アプリがこの行を購読ボタンに変換する）。
回答は簡潔に。コードの編集やファイル操作は行わないこと。"#;

impl PiBridge {
    pub fn new() -> Self {
        Self { inner: Mutex::new(None) }
    }

    /// Spawn pi if not running, wiring stdout/stderr to `pi-event` / `pi-error` events.
    /// Returns a clone of the stdin handle.
    fn ensure_spawned(&self, app: &AppHandle) -> Result<Arc<tokio::sync::Mutex<ChildStdin>>, String> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(proc) = guard.as_mut() {
            if proc.child.try_wait().map(|s| s.is_none()).unwrap_or(false) {
                return Ok(proc.stdin.clone());
            }
            *guard = None; // process died; respawn below
        }

        let mut child = Command::new("pi")
            .args([
                "--mode",
                "rpc",
                "--no-session",
                "--no-context-files",
                "--tools",
                "bash",
                "--append-system-prompt",
                SYSTEM_PROMPT,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("piを起動できません（`pi`コマンドがPATHにありますか？）: {e}"))?;

        let stdin = child.stdin.take().ok_or("piのstdinを取得できません")?;
        let stdout = child.stdout.take().ok_or("piのstdoutを取得できません")?;
        let stderr = child.stderr.take().ok_or("piのstderrを取得できません")?;

        let app_out = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = app_out.emit("pi-event", &line);
            }
            let _ = app_out.emit("pi-closed", ());
        });
        let app_err = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = app_err.emit("pi-error", &line);
            }
        });

        let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
        *guard = Some(PiProc { child, stdin: stdin.clone() });
        Ok(stdin)
    }

    async fn write_line(&self, app: &AppHandle, value: serde_json::Value) -> Result<(), String> {
        let stdin = self.ensure_spawned(app)?;
        let mut stdin = stdin.lock().await;
        let line = format!("{}\n", value);
        stdin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn prompt(&self, app: &AppHandle, message: &str) -> Result<(), String> {
        self.write_line(app, json!({"type": "prompt", "message": message})).await
    }

    pub async fn abort(&self, app: &AppHandle) -> Result<(), String> {
        self.write_line(app, json!({"type": "abort"})).await
    }

    pub async fn new_session(&self, app: &AppHandle) -> Result<(), String> {
        self.write_line(app, json!({"type": "new_session"})).await
    }
}
