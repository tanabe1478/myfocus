import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Article } from "../types";
import { relativeTime } from "../format";

interface Props {
  articles: Article[];
  selectedId: number | null;
  title: string;
  onSelect: (article: Article) => void;
  onMarkAllRead?: () => void;
  showSummaryStatus?: boolean;
}

export function ArticleList({
  articles,
  selectedId,
  title,
  onSelect,
  onMarkAllRead,
  showSummaryStatus = false,
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
        {onMarkAllRead && (
          <button className="icon-button" title="すべて既読にする" onClick={onMarkAllRead}>
            ✓
          </button>
        )}
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
                data-article-id={a.id}
                data-testid={`article-${a.id}`}
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
                  {showSummaryStatus && a.ai_summary_status && (
                    <span className={`summary-job-status ${a.ai_summary_status}`}>
                      {a.ai_summary_status === "queued"
                        ? "待機中"
                        : a.ai_summary_status === "running"
                          ? "生成中"
                          : a.ai_summary_status === "failed"
                            ? "失敗"
                            : a.ai_summary_reviewed
                              ? "確認済み"
                              : "新着"}
                    </span>
                  )}
                  <span className="article-row-time">{relativeTime(a.published_at)}</span>
                </div>
                <div className="article-row-title">
                  {a.starred && <span className="star">★ </span>}
                  {a.title || "(無題)"}
                </div>
                {(showSummaryStatus ? a.ai_summary || a.summary : a.summary) && (
                  <div className="article-row-summary">
                    {showSummaryStatus ? a.ai_summary || a.summary : a.summary}
                  </div>
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
