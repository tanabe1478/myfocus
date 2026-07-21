import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getSetting } from "./api";

let enabled = false;
let initialized = false;

function errorDetails(value: unknown): Record<string, string> {
  if (value instanceof Error) {
    return { name: value.name, message: value.message, stack: value.stack ?? "" };
  }
  return { message: String(value) };
}

export async function logDiagnostic(
  level: "info" | "warn" | "error" | "debug",
  event: string,
  details?: unknown,
): Promise<void> {
  if (!enabled) return;
  await invoke("diagnostic_log", { level, event, details: details ?? null }).catch(() => {});
}

export function initializeDiagnostics(windowName: "main" | "settings"): void {
  if (initialized) return;
  initialized = true;

  getSetting("diagnostic_logging_enabled")
    .then((value) => {
      enabled = value === "true";
      return logDiagnostic("info", "webview_started", { window: windowName });
    })
    .catch(() => {});

  void listen<string>("settings-updated", (event) => {
    if (event.payload !== "diagnostic_logging_enabled") return;
    getSetting("diagnostic_logging_enabled")
      .then((value) => {
        enabled = value === "true";
        return logDiagnostic("info", "webview_logging_state_changed", {
          window: windowName,
          enabled,
        });
      })
      .catch(() => {});
  });

  window.addEventListener("error", (event) => {
    void logDiagnostic("error", "frontend_error", {
      window: windowName,
      message: event.message,
      source: event.filename,
      line: event.lineno,
      column: event.colno,
      ...errorDetails(event.error),
    });
  });
  window.addEventListener("unhandledrejection", (event) => {
    void logDiagnostic("error", "unhandled_promise_rejection", {
      window: windowName,
      ...errorDetails(event.reason),
    });
  });
}
