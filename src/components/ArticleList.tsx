import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Article } from "../types";
import { relativeTime } from "../format";

interface Props {
  articles: Article[];
  selectedId: number | null;
  title: string;
  /** 日本語ダイジェストが有効なフィードのID集合 */
  translatingFeedIds: Set<number>;
  onSelect: (article: Article) => void;
  onMarkAllRead: () => void;
}

export function ArticleList({
  articles,
  selectedId,
  title,
  translatingFeedIds,
  onSelect,
  onMarkAllRead,
}: Props) {
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: articles.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 84,
    overscan: 12,
  });

  // keep the selected row visible during j/k keyboard navigation
  useEffect(() => {
    if (selectedId == null) return;
    const index = articles.findIndex((a) => a.id === selectedId);
    if (index >= 0) virtualizer.scrollToIndex(index, { align: "auto" });
  }, [selectedId, articles, virtualizer]);

  return (
    <section className="article-list">
      <div className="pane-header">
        <span className="pane-title" title={title}>
          {title}
        </span>
        <button className="icon-button" title="すべて既読にする" onClick={onMarkAllRead}>
          ✓
        </button>
      </div>
      <div className="article-rows" ref={parentRef}>
        <div
          style={{ height: virtualizer.getTotalSize(), width: "100%", position: "relative" }}
        >
          {virtualizer.getVirtualItems().map((vi) => {
            const a = articles[vi.index];
            return (
              <div
                key={a.id}
                data-index={vi.index}
                ref={virtualizer.measureElement}
                className={`article-row ${a.id === selectedId ? "selected" : ""} ${a.read ? "read" : ""}`}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${vi.start}px)`,
                }}
                onClick={() => onSelect(a)}
              >
                <div className="article-row-top">
                  {!a.read && <span className="unread-dot" />}
                  <span className="article-row-feed">{a.feed_title}</span>
                  {!a.title_ja && translatingFeedIds.has(a.feed_id) && (
                    <span className="translate-pending">翻訳待ち</span>
                  )}
                  <span className="article-row-time">{relativeTime(a.published_at)}</span>
                </div>
                <div className="article-row-title">
                  {a.starred && <span className="star">★ </span>}
                  {a.title_ja || a.title || "(無題)"}
                </div>
                {a.title_ja && a.title_ja !== a.title && (
                  <div className="article-row-original">{a.title}</div>
                )}
                {(a.summary_ja || a.summary) && (
                  <div className="article-row-summary">{a.summary_ja ?? a.summary}</div>
                )}
              </div>
            );
          })}
        </div>
        {articles.length === 0 && <div className="empty-hint">記事はありません</div>}
      </div>
    </section>
  );
}
