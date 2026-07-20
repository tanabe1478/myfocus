import { useMemo, useRef, useState } from "react";
import type { Feed, SavedSearch, Selection } from "../types";
import { openSettings } from "../api";

const COLLAPSED_KEY = "myfocus.collapsedCategories";

function loadCollapsed(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]"));
  } catch {
    return new Set();
  }
}

interface Props {
  feeds: Feed[];
  selection: Selection;
  savedSearches: SavedSearch[];
  totalUnread: number;
  refreshing: boolean;
  onSelect: (sel: Selection) => void;
  onRemoveSavedSearch: (searchId: string) => void;
  onAddFeed: (url: string) => Promise<void>;
  onRemoveFeed: (feedId: number) => void;
  onRefresh: () => void;
  onImportOpml: (content: string) => Promise<number>;
}

export function Sidebar({
  feeds,
  selection,
  savedSearches,
  totalUnread,
  refreshing,
  onSelect,
  onRemoveSavedSearch,
  onAddFeed,
  onRemoveFeed,
  onRefresh,
  onImportOpml,
}: Props) {
  const [adding, setAdding] = useState(false);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [importNote, setImportNote] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);
  const fileRef = useRef<HTMLInputElement>(null);

  const groups = useMemo(() => {
    const uncategorized: Feed[] = [];
    const byCategory = new Map<string, Feed[]>();
    for (const f of feeds) {
      if (f.category) {
        const list = byCategory.get(f.category) ?? [];
        list.push(f);
        byCategory.set(f.category, list);
      } else {
        uncategorized.push(f);
      }
    }
    return { uncategorized, byCategory };
  }, [feeds]);

  const toggleCollapse = (category: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(category)) {
        next.delete(category);
      } else {
        next.add(category);
      }
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
      return next;
    });
  };

  const handleOpmlFile = async (file: File) => {
    try {
      const content = await file.text();
      const added = await onImportOpml(content);
      setImportNote(`${added}件のフィードを追加しました（記事を取得中…）`);
      setTimeout(() => setImportNote(null), 8000);
    } catch (e) {
      setImportNote(`インポート失敗: ${e}`);
    }
  };

  const isSelected = (sel: Selection) => JSON.stringify(sel) === JSON.stringify(selection);

  const submit = async () => {
    const trimmed = url.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onAddFeed(trimmed);
      setUrl("");
      setAdding(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="app-name">myfocus</span>
        <span>
          <button
            className="icon-button"
            data-testid="open-settings"
            title="設定（⌘/Ctrl+,）"
            onClick={() => openSettings()}
          >
            ⚙
          </button>
          <button
            className="icon-button"
            title="すべて更新"
            onClick={onRefresh}
            disabled={refreshing}
          >
            {refreshing ? "…" : "⟳"}
          </button>
        </span>
      </div>

      <nav className="smart-list">
        <SidebarRow
          label="すべて"
          selected={isSelected({ kind: "all" })}
          onClick={() => onSelect({ kind: "all" })}
        />
        <SidebarRow
          label="未読"
          count={totalUnread}
          selected={isSelected({ kind: "unread" })}
          onClick={() => onSelect({ kind: "unread" })}
        />
        <SidebarRow
          label="スター付き"
          selected={isSelected({ kind: "starred" })}
          onClick={() => onSelect({ kind: "starred" })}
        />
      </nav>

      <div className="section-label">ソース</div>
      <nav className="smart-list">
        <SidebarRow
          label="Hacker News"
          selected={isSelected({ kind: "hn" })}
          onClick={() => onSelect({ kind: "hn" })}
        />
      </nav>

      {savedSearches.length > 0 && (
        <>
          <div className="section-label">保存した検索</div>
          <nav className="smart-list saved-search-list">
            {savedSearches.map((search) => {
              const target: Selection = {
                kind: "search",
                searchId: search.id,
                name: search.name,
                query: search.query,
              };
              return (
                <SidebarRow
                  key={search.id}
                  label={`⌕ ${search.name}`}
                  selected={isSelected(target)}
                  onClick={() => onSelect(target)}
                  onRemove={() => onRemoveSavedSearch(search.id)}
                />
              );
            })}
          </nav>
        </>
      )}

      <div className="section-label">
        フィード
        <span>
          <button
            className="icon-button"
            title="OPMLをインポート"
            onClick={() => fileRef.current?.click()}
          >
            ⇪
          </button>
          <button className="icon-button" title="フィードを追加" onClick={() => setAdding(!adding)}>
            ＋
          </button>
        </span>
      </div>

      <input
        ref={fileRef}
        type="file"
        accept=".xml,.opml"
        style={{ display: "none" }}
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) handleOpmlFile(file);
          e.target.value = "";
        }}
      />
      {importNote && <div className="add-feed-status">{importNote}</div>}

      {adding && (
        <div className="add-feed">
          <input
            autoFocus
            placeholder="フィードまたはサイトのURL"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
              if (e.key === "Escape") setAdding(false);
            }}
            disabled={busy}
          />
          {busy && <div className="add-feed-status">取得中…</div>}
          {error && <div className="add-feed-error">{error}</div>}
        </div>
      )}

      <div className="feed-list">
        {groups.uncategorized.map((f) => (
          <FeedRow
            key={f.id}
            feed={f}
            isSelected={isSelected}
            onSelect={onSelect}
            onRemoveFeed={onRemoveFeed}
          />
        ))}
        {[...groups.byCategory.entries()].map(([category, list]) => {
          const isCollapsed = collapsed.has(category);
          const unread = list.reduce((sum, f) => sum + f.unread_count, 0);
          return (
            <div key={category} className="feed-group">
              <div
                className={`sidebar-row category-row ${
                  isSelected({ kind: "category", category }) ? "selected" : ""
                }`}
                onClick={() => onSelect({ kind: "category", category })}
              >
                <button
                  className="icon-button category-toggle"
                  title={isCollapsed ? "展開" : "折りたたむ"}
                  onClick={(e) => {
                    e.stopPropagation();
                    toggleCollapse(category);
                  }}
                >
                  {isCollapsed ? "▸" : "▾"}
                </button>
                <span className="sidebar-row-label" title={category}>
                  {category}
                </span>
                {unread > 0 && <span className="unread-badge">{unread}</span>}
              </div>
              {!isCollapsed &&
                list.map((f) => (
                  <FeedRow
                    key={f.id}
                    feed={f}
                    indent
                    isSelected={isSelected}
                    onSelect={onSelect}
                    onRemoveFeed={onRemoveFeed}
                          />
                ))}
            </div>
          );
        })}
        {feeds.length === 0 && (
          <div className="empty-hint">＋ からフィードを追加してください</div>
        )}
      </div>
    </aside>
  );
}

function FeedRow({
  feed,
  indent,
  isSelected,
  onSelect,
  onRemoveFeed,
}: {
  feed: Feed;
  indent?: boolean;
  isSelected: (sel: Selection) => boolean;
  onSelect: (sel: Selection) => void;
  onRemoveFeed: (feedId: number) => void;
}) {
  return (
    <div className={indent ? "feed-row-indent" : undefined}>
      <SidebarRow
        label={feed.title || feed.url}
        count={feed.unread_count}
        error={feed.last_error}
        selected={isSelected({ kind: "feed", feedId: feed.id })}
        onClick={() => onSelect({ kind: "feed", feedId: feed.id })}
        onRemove={() => {
          if (confirm(`「${feed.title}」の購読を解除しますか？記事も削除されます。`)) {
            onRemoveFeed(feed.id);
          }
        }}
      />
    </div>
  );
}

function SidebarRow({
  label,
  count,
  error,
  selected,
  onClick,
  onRemove,
}: {
  label: string;
  count?: number;
  error?: string | null;
  selected: boolean;
  onClick: () => void;
  onRemove?: () => void;
}) {
  return (
    <div
      className={`sidebar-row ${selected ? "selected" : ""} ${error ? "has-error" : ""}`}
      onClick={onClick}
    >
      <span className="sidebar-row-label" title={label}>
        {label}
      </span>
      {error && (
        <span className="feed-error-badge" title={`取得エラー: ${error}`}>
          ⚠
        </span>
      )}
      {onRemove && (
        <button
          className="icon-button row-remove"
          title="購読解除"
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
        >
          ×
        </button>
      )}
      {count != null && count > 0 && <span className="unread-badge">{count}</span>}
    </div>
  );
}
