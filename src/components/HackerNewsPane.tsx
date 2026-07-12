import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { HnItem } from "../types";
import { hnList, hnRefresh, hnSummarizeComments, openBackground } from "../api";

/**
 * Hacker News専用ビュー。RSSの3ペインとは独立した1カラムのダイジェストUI。
 * フロントページの各ストーリーを日本語タイトル+AIダイジェストで読み流し、
 * 必要に応じてコメント要約を生成する。
 */
export function HackerNewsPane() {
  const [items, setItems] = useState<HnItem[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [status, setStatus] = useState({ active: false, remaining: 0 });
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setItems(await hnList());
  }, []);

  useEffect(() => {
    reload();
    const unlistenUpdated = listen("hn-updated", reload);
    const unlistenStatus = listen<{ active: boolean; remaining: number }>(
      "hn-status",
      (e) => setStatus(e.payload)
    );
    const unlistenError = listen<string>("hn-error", (e) => {
      setError(e.payload);
      setTimeout(() => setError(null), 10000);
    });
    return () => {
      unlistenUpdated.then((f) => f());
      unlistenStatus.then((f) => f());
      unlistenError.then((f) => f());
    };
  }, [reload]);

  const refresh = async () => {
    setRefreshing(true);
    setError(null);
    try {
      await hnRefresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <section className="hn-pane">
      <div className="pane-header">
        <span className="pane-title">Hacker News</span>
        {status.active && (
          <span className="hn-status">翻訳中… 残り{status.remaining}件</span>
        )}
        <button
          className="icon-button"
          title="フロントページを再取得"
          onClick={refresh}
          disabled={refreshing}
        >
          {refreshing ? "…" : "⟳"}
        </button>
      </div>
      {error && <div className="hn-error">{error}</div>}
      <div className="hn-list">
        {items.map((item) => (
          <HnStory key={item.id} item={item} />
        ))}
        {items.length === 0 && (
          <div className="empty-hint">
            {refreshing ? "取得中…" : "⟳ でHacker Newsのフロントページを取得します"}
          </div>
        )}
      </div>
    </section>
  );
}

function HnStory({ item }: { item: HnItem }) {
  const titleJa = item.digest?.title_ja;
  const openStory = () => {
    const url = item.url ?? item.comments_url;
    openUrl(url);
  };

  return (
    <article className="hn-story">
      <div className="hn-story-title" onClick={openStory}>
        {titleJa ?? item.title}
        {!titleJa && <span className="translate-pending">翻訳待ち</span>}
      </div>
      {titleJa && titleJa !== item.title && (
        <div className="hn-story-original">{item.title}</div>
      )}
      <div className="hn-story-meta">
        <span>▲ {item.points}</span>
        <a
          href={item.comments_url}
          onClick={(e) => {
            e.preventDefault();
            openUrl(item.comments_url);
          }}
        >
          {item.comments_count} comments
        </a>
        {item.url && (
          <>
            <span className="hn-story-domain">{domainOf(item.url)}</span>
            <button
              className="hn-open-bg"
              title="バックグラウンドでブラウザで開く"
              onClick={() => openBackground(item.url!)}
            >
              ⧉
            </button>
          </>
        )}
      </div>
      {item.digest?.summary_ja && (
        <div className="hn-story-digest">
          {item.digest.summary_ja.split("\n\n").map((p, i) => (
            <p key={i}>{p}</p>
          ))}
        </div>
      )}
      <HnComments item={item} />
    </article>
  );
}

function HnComments({ item }: { item: HnItem }) {
  const [summary, setSummary] = useState<string | null>(
    item.digest?.comments_summary_ja ?? null
  );
  const [open, setOpen] = useState(!!item.digest?.comments_summary_ja);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const cached = item.digest?.comments_summary_ja ?? null;
    setSummary(cached);
    setOpen(!!cached);
  }, [item.id, item.digest?.comments_summary_ja]);

  const load = async () => {
    if (summary) {
      setOpen((v) => !v);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setSummary(await hnSummarizeComments(item.id));
      setOpen(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  if (item.comments_count === 0 && !summary) return null;

  return (
    <div className="hn-comments">
      <button className="hn-comments-toggle" onClick={load} disabled={loading}>
        {loading
          ? "コメントを読んで要約しています…"
          : summary
            ? open
              ? "コメント要約を隠す"
              : "コメント要約を表示"
            : "コメントを要約"}
      </button>
      {error && <div className="hn-error">{error}</div>}
      {open && summary && (
        <div className="hn-comments-box">
          {summary.split("\n").map((line, i) => (
            <p key={i}>{line}</p>
          ))}
        </div>
      )}
    </div>
  );
}

function domainOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}
