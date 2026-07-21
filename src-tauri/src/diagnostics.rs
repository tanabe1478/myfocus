use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const LOG_FILE_NAME: &str = "myfocus-diagnostic.jsonl";
const BACKUP_FILE_NAME: &str = "myfocus-diagnostic.1.jsonl";

pub struct DiagnosticLogger {
    directory: PathBuf,
    enabled: AtomicBool,
    file: Mutex<Option<File>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticInfo {
    pub enabled: bool,
    pub directory: String,
    pub file: String,
    pub size_bytes: u64,
}

impl DiagnosticLogger {
    pub fn new(directory: PathBuf, enabled: bool) -> Self {
        let logger = Self {
            directory,
            enabled: AtomicBool::new(enabled),
            file: Mutex::new(None),
        };
        if enabled {
            logger.log("info", "logger_started", None);
        }
        logger
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        if enabled == self.enabled.swap(enabled, Ordering::SeqCst) {
            return;
        }
        if enabled {
            self.log("info", "logging_enabled", None);
        } else {
            // The flag is already false, so write the final record directly.
            self.write_record("info", "logging_disabled", None);
            if let Ok(mut file) = self.file.lock() {
                *file = None;
            }
        }
    }

    pub fn log(&self, level: &str, event: &str, details: Option<Value>) {
        if self.is_enabled() {
            self.write_record(level, event, details);
        }
    }

    fn write_record(&self, level: &str, event: &str, details: Option<Value>) {
        let record = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "level": normalize_level(level),
            "event": truncate(event, 120),
            "details": details.map(limit_value),
            "pid": std::process::id(),
            "platform": std::env::consts::OS,
            "version": env!("CARGO_PKG_VERSION"),
        });
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        if self.log_path().metadata().map(|m| m.len()).unwrap_or(0) >= MAX_LOG_BYTES {
            *guard = None;
            if self.rotate().is_err() {
                return;
            }
        }
        if guard.is_none() {
            if fs::create_dir_all(&self.directory).is_err() {
                return;
            }
            *guard = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.log_path())
                .ok();
        }
        if let Some(file) = guard.as_mut() {
            if serde_json::to_writer(&mut *file, &record).is_ok() {
                let _ = file.write_all(b"\n");
                let _ = file.flush();
            }
        }
    }

    fn rotate(&self) -> std::io::Result<()> {
        let path = self.log_path();
        let backup = self.directory.join(BACKUP_FILE_NAME);
        let _ = fs::remove_file(backup.as_path());
        fs::rename(path, backup)
    }

    pub fn info(&self) -> DiagnosticInfo {
        let path = self.log_path();
        DiagnosticInfo {
            enabled: self.is_enabled(),
            directory: self.directory.to_string_lossy().into_owned(),
            file: path.to_string_lossy().into_owned(),
            size_bytes: path.metadata().map(|m| m.len()).unwrap_or(0),
        }
    }

    pub fn clear(&self) -> std::io::Result<()> {
        if let Ok(mut file) = self.file.lock() {
            *file = None;
        }
        for name in [LOG_FILE_NAME, BACKUP_FILE_NAME] {
            match fs::remove_file(self.directory.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn directory(&self) -> &PathBuf {
        &self.directory
    }

    fn log_path(&self) -> PathBuf {
        self.directory.join(LOG_FILE_NAME)
    }
}

fn normalize_level(level: &str) -> &'static str {
    match level.to_ascii_lowercase().as_str() {
        "error" => "error",
        "warn" | "warning" => "warn",
        "debug" => "debug",
        _ => "info",
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn limit_value(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(truncate(&text, 8_000)),
        other => {
            let text = other.to_string();
            if text.chars().count() <= 8_000 {
                other
            } else {
                Value::String(truncate(&text, 8_000))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DiagnosticLogger;

    #[test]
    fn disabled_logger_does_not_create_a_file() {
        let dir = std::env::temp_dir().join(format!("myfocus-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logger = DiagnosticLogger::new(dir.clone(), false);
        logger.log("error", "ignored", None);
        assert!(!logger.info().file.is_empty());
        assert_eq!(logger.info().size_bytes, 0);
        assert!(!dir.exists());
    }
}
