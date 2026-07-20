import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AiMessage } from "../types";

interface Props {
  messages: AiMessage[];
  busy: boolean;
  status: string | null;
  onSend: (text: string) => void;
  onRecommend: (force?: boolean) => void;
  onAbort: () => void;
  onReset: () => void;
  onSubscribe: (url: string) => void;
  onOpenArticle: (articleId: number) => void;
  feedback: Record<string, number>;
  onFeedback: (articleId: number, value: -1 | 0 | 1) => void;
  onClose: () => void;
}

const FEED_LINE = /^FEED:\s*(https?:\/\/\S+)\s*$/;
const ARTICLE_LINE = /^ARTICLE:\s*(\d+)\s*\|\s*(.+)\s*$/;
const URL_PATTERN = /https?:\/\/[^\s<>"')\]]+/g;

/// テキスト中のURLを外部ブラウザで開くリンクに変換する
function linkify(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(URL_PATTERN)) {
    const url = m[0];
    if (m.index > last) nodes.push(text.slice(last, m.index));
    nodes.push(
      <a
        key={`${m.index}-${url}`}
        href={url}
        className="ai-link"
        onClick={(e) => {
          e.preventDefault();
          openUrl(url);
        }}
      >
        {url}
      </a>
    );
    last = m.index + url.length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

export function AiPanel({
  messages,
  busy,
  status,
  onSend,
  onRecommend,
  onAbort,
  onReset,
  onSubscribe,
  onOpenArticle,
  feedback,
  onFeedback,
  onClose,
}: Props) {
  const [input, setInput] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages, status]);

  const submit = () => {
    if (busy || !input.trim()) return;
    onSend(input);
    setInput("");
  };

  return (
    <aside className="ai-panel">
      <div className="pane-header">
        <span className="pane-title">✦ アシスタント</span>
        <button
          className="icon-button"
          title="会話をリセットして最初から"
          onClick={() => {
            if (messages.length === 0 || confirm("会話をリセットして最初から始めますか？")) {
              onReset();
            }
          }}
        >
          ⊕
        </button>
        <button className="icon-button" title="閉じる" onClick={onClose}>
          ×
        </button>
      </div>

      <div className="ai-messages" ref={scrollRef}>
        {messages.length === 0 && (
          <div className="ai-hint">
            <p>
              購読記事を横断して探したり、未読の整理や読んでいる記事について相談できます。必要ならWebから新しい記事やフィードも探します。
            </p>
            <div className="ai-suggestions">
              <button onClick={() => onRecommend(false)}>
                今日読む記事を選んで
              </button>
              <button onClick={() => onRecommend(true)}>
                おすすめを再生成
              </button>
              <button onClick={() => onSend("保存してある記事から、最近のAI関連の記事を探して要点を教えて")}>
                保存記事からAI関連を探す
              </button>
              <button onClick={() => onSend("現在の記事数、未読数、よく購読している分野を教えて")}>
                記事アーカイブの状況
              </button>
              <button onClick={() => onSend("Rust関連の技術ブログのRSSフィードをWebからいくつか探して")}>
                新しいフィードをWebで探す
              </button>
            </div>
          </div>
        )}
        {messages.map((m, i) => (
          <MessageBubble
            key={i}
            message={m}
            onSubscribe={onSubscribe}
            onOpenArticle={onOpenArticle}
            feedback={feedback}
            onFeedback={onFeedback}
          />
        ))}
        {status && <div className="ai-status">{status}</div>}
        {busy && !status && <div className="ai-status">考えています…</div>}
      </div>

      <div className="ai-input-row">
        <textarea
          className="ai-input"
          placeholder="保存記事を検索・相談する…（⌘Enterで送信）"
          value={input}
          rows={2}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            // Enterは改行。⌘Enter / Shift+Enterで送信
            if (
              e.key === "Enter" &&
              (e.metaKey || e.shiftKey) &&
              !e.nativeEvent.isComposing
            ) {
              e.preventDefault();
              submit();
            }
          }}
        />
        {busy ? (
          <button className="ai-send" onClick={onAbort}>
            停止
          </button>
        ) : (
          <button className="ai-send" onClick={submit} disabled={!input.trim()}>
            送信
          </button>
        )}
      </div>
    </aside>
  );
}

function MessageBubble({
  message,
  onSubscribe,
  onOpenArticle,
  feedback,
  onFeedback,
}: {
  message: AiMessage;
  onSubscribe: (url: string) => void;
  onOpenArticle: (articleId: number) => void;
  feedback: Record<string, number>;
  onFeedback: (articleId: number, value: -1 | 0 | 1) => void;
}) {
  const lines = message.text.split("\n");
  return (
    <div className={`ai-message ${message.role}`}>
      {lines.map((line, i) => {
        const article = line.match(ARTICLE_LINE);
        if (article && message.role === "assistant") {
          const articleId = Number(article[1]);
          const value = feedback[String(articleId)] ?? 0;
          return (
            <div key={i} className="article-suggestion">
              <span className="article-suggestion-title">{article[2]}</span>
              <button
                className={value === 1 ? "feedback-active" : ""}
                title="興味あり"
                onClick={() => onFeedback(articleId, value === 1 ? 0 : 1)}
              >
                👍
              </button>
              <button
                className={value === -1 ? "feedback-active negative" : ""}
                title="興味なし"
                onClick={() => onFeedback(articleId, value === -1 ? 0 : -1)}
              >
                👎
              </button>
              <button onClick={() => onOpenArticle(articleId)}>記事を開く</button>
            </div>
          );
        }
        const feed = line.match(FEED_LINE);
        if (feed && message.role === "assistant") {
          return (
            <div key={i} className="feed-suggestion">
              <span className="feed-suggestion-url">{feed[1]}</span>
              <button onClick={() => onSubscribe(feed[1])}>購読</button>
            </div>
          );
        }
        return <div key={i}>{line ? linkify(line) : " "}</div>;
      })}
    </div>
  );
}
