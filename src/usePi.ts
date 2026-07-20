import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { aiAbort, aiNewSession, aiPrompt, getSetting, setSetting } from "./api";
import type { AiMessage } from "./types";

const CONVERSATION_KEY = "ai_conversation";
const RECOMMENDATION_CACHE_KEY = "ai_recommendation_cache";
const RECOMMENDATION_CACHE_MS = 12 * 60 * 60 * 1000;
const RECOMMENDATION_PROMPT =
  "スター、興味あり／なし、最近読んだ記事の傾向も参考に、未読記事から今日読むべきものを5件選んで理由と一緒に教えて";
// piに文脈として渡す履歴の上限（文字数）
const CONTEXT_CHAR_LIMIT = 8000;

/// 復元した会話履歴を、新しいpiプロセスへの最初のプロンプトに添える形に整形する
function buildContextPrefix(history: AiMessage[]): string {
  const lines: string[] = [];
  let total = 0;
  for (let i = history.length - 1; i >= 0; i--) {
    const m = history[i];
    const line = `${m.role === "user" ? "ユーザー" : "アシスタント"}: ${m.text}`;
    if (total + line.length > CONTEXT_CHAR_LIMIT) break;
    lines.unshift(line);
    total += line.length;
  }
  return (
    "（システム: 以下はアプリ再起動前までの会話履歴です。文脈として引き継いで応答してください。履歴への返答は不要です）\n" +
    lines.join("\n---\n") +
    "\n（履歴ここまで）\n\n"
  );
}

export function usePi() {
  const [messages, setMessages] = useState<AiMessage[]>([]);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const hadTurn = useRef(false);
  const stderrLines = useRef<string[]>([]);
  // piプロセスが会話の記憶を持っていない状態（起動直後・プロセス死亡後）
  const needsContext = useRef(false);
  const messagesRef = useRef<AiMessage[]>([]);
  const pendingRecommendation = useRef(false);
  const assistantText = useRef("");
  messagesRef.current = messages;

  // 保存済みの会話を復元する
  useEffect(() => {
    getSetting(CONVERSATION_KEY)
      .then((v) => {
        if (!v) return;
        const saved = JSON.parse(v) as AiMessage[];
        if (Array.isArray(saved) && saved.length > 0) {
          setMessages((cur) => (cur.length === 0 ? saved : cur));
          needsContext.current = true;
        }
      })
      .catch(() => {});
  }, []);

  // 会話をDBに保存する（応答ストリーミング中は書きすぎないようデバウンス）
  useEffect(() => {
    const timer = setTimeout(() => {
      setSetting(CONVERSATION_KEY, JSON.stringify(messages)).catch(() => {});
    }, 500);
    return () => clearTimeout(timer);
  }, [messages]);

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
          assistantText.current = "";
          setMessages((m) => [...m, { role: "assistant", text: "" }]);
          break;
        case "turn_start":
          // separate turns (before/after tool calls) with a blank line
          if (hadTurn.current && assistantText.current) {
            assistantText.current += "\n\n";
          }
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
            assistantText.current += delta.delta;
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
              ? "記事を検索・確認しています…"
              : `ツール実行中: ${ev.toolName}`
          );
          break;
        case "tool_execution_end":
          setStatus(null);
          break;
        case "agent_end":
          setBusy(false);
          setStatus(null);
          if (pendingRecommendation.current && assistantText.current.trim()) {
            setSetting(
              RECOMMENDATION_CACHE_KEY,
              JSON.stringify({
                createdAt: Date.now(),
                text: assistantText.current.trim(),
              })
            ).catch(() => {});
          }
          pendingRecommendation.current = false;
          // drop an empty assistant bubble (e.g. aborted before any text)
          setMessages((m) => {
            const last = m[m.length - 1];
            if (last?.role === "assistant" && !last.text.trim()) return m.slice(0, -1);
            return m;
          });
          break;
      }
    });
    const unlistenErr = listen<string>("pi-error", (e) => {
      stderrLines.current = [...stderrLines.current.slice(-9), e.payload];
    });
    const unlistenClosed = listen("pi-closed", () => {
      setBusy(false);
      setStatus(null);
      pendingRecommendation.current = false;
      // piが返答なしで終了した場合はstderrをエラーとして表示する
      setMessages((m) => {
        const last = m[m.length - 1];
        const silent =
          last?.role === "user" || (last?.role === "assistant" && !last.text.trim());
        if (!silent) return m;
        const detail = stderrLines.current.slice(-5).join("\n");
        const base = m[m.length - 1]?.role === "assistant" ? m.slice(0, -1) : m;
        return [
          ...base,
          {
            role: "assistant",
            text: `AIプロセスが終了しました。${detail ? `\n${detail}` : ""}`,
          },
        ];
      });
      stderrLines.current = [];
      // プロセスが死んだので、次の送信時に履歴を文脈として渡し直す
      needsContext.current = true;
    });
    return () => {
      unlisten.then((f) => f());
      unlistenErr.then((f) => f());
      unlistenClosed.then((f) => f());
    };
  }, []);

  const send = useCallback(async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed) return;
    const history = messagesRef.current;
    // piが記憶を持たない状態なら、復元済み履歴を文脈として先頭に添える
    const prompt =
      needsContext.current && history.length > 0
        ? buildContextPrefix(history) + trimmed
        : trimmed;
    needsContext.current = false;
    setMessages((m) => [...m, { role: "user", text: trimmed }]);
    try {
      await aiPrompt(prompt);
    } catch (err) {
      pendingRecommendation.current = false;
      setMessages((m) => [...m, { role: "assistant", text: `エラー: ${err}` }]);
      setBusy(false);
    }
  }, []);

  const recommend = useCallback(async (force = false) => {
    if (!force) {
      try {
        const raw = await getSetting(RECOMMENDATION_CACHE_KEY);
        if (raw) {
          const cached = JSON.parse(raw) as { createdAt?: number; text?: string };
          if (
            typeof cached.createdAt === "number" &&
            typeof cached.text === "string" &&
            cached.text.trim() &&
            Date.now() - cached.createdAt < RECOMMENDATION_CACHE_MS
          ) {
            setMessages((m) => [
              ...m,
              { role: "user", text: RECOMMENDATION_PROMPT },
              { role: "assistant", text: cached.text! },
            ]);
            return;
          }
        }
      } catch {
        // Ignore malformed or unavailable caches and generate a fresh response.
      }
    }
    pendingRecommendation.current = true;
    await send(RECOMMENDATION_PROMPT);
  }, [send]);

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
    needsContext.current = false;
    pendingRecommendation.current = false;
    setSetting(CONVERSATION_KEY, "[]").catch(() => {});
    try {
      await aiNewSession();
    } catch {
      /* not started yet */
    }
  }, []);

  return { messages, busy, status, send, recommend, abort, reset };
}
