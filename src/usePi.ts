import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { aiAbort, aiNewSession, aiPrompt } from "./api";
import type { AiMessage } from "./types";

export function usePi() {
  const [messages, setMessages] = useState<AiMessage[]>([]);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const hadTurn = useRef(false);

  useEffect(() => {
    const unlisten = listen<string>("pi-event", (e) => {
      let ev: any;
      try {
        ev = JSON.parse(e.payload);
      } catch {
        return;
      }
      switch (ev.type) {
        case "agent_start":
          setBusy(true);
          hadTurn.current = false;
          setMessages((m) => [...m, { role: "assistant", text: "" }]);
          break;
        case "turn_start":
          // separate turns (before/after tool calls) with a blank line
          if (hadTurn.current) {
            setMessages((m) => {
              const last = m[m.length - 1];
              if (last?.role === "assistant" && last.text) {
                return [...m.slice(0, -1), { ...last, text: last.text + "\n\n" }];
              }
              return m;
            });
          }
          hadTurn.current = true;
          break;
        case "message_update": {
          const delta = ev.assistantMessageEvent;
          if (delta?.type === "text_delta" && typeof delta.delta === "string") {
            setMessages((m) => {
              const last = m[m.length - 1];
              if (last?.role !== "assistant") return m;
              return [...m.slice(0, -1), { ...last, text: last.text + delta.delta }];
            });
          }
          break;
        }
        case "tool_execution_start":
          setStatus(
            ev.toolName === "bash"
              ? "Webを検索しています…"
              : `ツール実行中: ${ev.toolName}`
          );
          break;
        case "tool_execution_end":
          setStatus(null);
          break;
        case "agent_end":
          setBusy(false);
          setStatus(null);
          // drop an empty assistant bubble (e.g. aborted before any text)
          setMessages((m) => {
            const last = m[m.length - 1];
            if (last?.role === "assistant" && !last.text.trim()) return m.slice(0, -1);
            return m;
          });
          break;
      }
    });
    const unlistenClosed = listen("pi-closed", () => {
      setBusy(false);
      setStatus(null);
    });
    return () => {
      unlisten.then((f) => f());
      unlistenClosed.then((f) => f());
    };
  }, []);

  const send = useCallback(async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    setMessages((m) => [...m, { role: "user", text: trimmed }]);
    try {
      await aiPrompt(trimmed);
    } catch (err) {
      setMessages((m) => [...m, { role: "assistant", text: `エラー: ${err}` }]);
      setBusy(false);
    }
  }, []);

  const abort = useCallback(async () => {
    try {
      await aiAbort();
    } catch {
      /* already stopped */
    }
  }, []);

  const reset = useCallback(async () => {
    setMessages([]);
    setBusy(false);
    setStatus(null);
    try {
      await aiNewSession();
    } catch {
      /* not started yet */
    }
  }, []);

  return { messages, busy, status, send, abort, reset };
}
