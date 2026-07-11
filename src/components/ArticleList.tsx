import type { Article } from "../types";
import { relativeTime } from "../format";

interface Props {
  articles: Article[];
  selectedId: number | null;
  title: string;
  onSelect: (article: Article) => void;
  onMarkAllRead: () => void;
}

export function ArticleList({ articles, selectedId, title, onSelect, onMarkAllRead }: Props) {
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
      <div className="article-rows">
        {articles.map((a) => (
          <div
            key={a.id}
            className={`article-row ${a.id === selectedId ? "selected" : ""} ${a.read ? "read" : ""}`}
            onClick={() => onSelect(a)}
          >
            <div className="article-row-top">
              {!a.read && <span className="unread-dot" />}
              <span className="article-row-feed">{a.feed_title}</span>
              <span className="article-row-time">{relativeTime(a.published_at)}</span>
            </div>
            <div className="article-row-title">
              {a.starred && <span className="star">★ </span>}
              {a.title || "(無題)"}
            </div>
            {a.summary && <div className="article-row-summary">{a.summary}</div>}
          </div>
        ))}
        {articles.length === 0 && <div className="empty-hint">記事はありません</div>}
      </div>
    </section>
  );
}
