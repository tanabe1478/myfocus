import { useMemo, useRef, useState } from "react";
import type { Feed, SavedSearch, Selection, SummaryStats } from "../types";
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
  summaryStats: SummaryStats;
  refreshing: boolean;
  onSelect: (sel: Selection) => void;
  onRemoveSavedSearch: (searchId: string) => void;
  onAddFeed: (url: string, category: string | null) => Promise<void>;
  onRemoveFeed: (feedId: number) => void;
  onSetFeedCategory: (feedId: number, category: string | null) => Promise<void>;
  onRenameCategory: (oldName: string, newName: string) => Promise<void>;
  onRefresh: () => void;
  onOpenBriefing: () => void;
  onImportOpml: (content: string) => Promise<number>;
}

export function Sidebar({
  feeds,
  selection,
  savedSearches,
  totalUnread,
  summaryStats,
  refreshing,
  onSelect,
  onRemoveSavedSearch,
  onAddFeed,
  onRemoveFeed,
  onSetFeedCategory,
  onRenameCategory,
  onRefresh,
  onOpenBriefing,
  onImportOpml,
}: Props) {
  const [adding, setAdding] = useState(false);
  const [url, setUrl] = useState("");
  const [newFeedCategory, setNewFeedCategory] = useState("");
  const [dragTarget, setDragTarget] = useState<string | null>(null);
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
  const categories = useMemo(() => [...groups.byCategory.keys()], [groups]);

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
      await onAddFeed(trimmed, newFeedCategory.trim() || null);
      setUrl("");
      setNewFeedCategory("");
      setAdding(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const moveFeed = async (feedId: number, category: string | null) => {
    try {
      await onSetFeedCategory(feedId, category);
    } catch (e) {
      setImportNote(`カテゴリ変更失敗: ${e}`);
    } finally {
      setDragTarget(null);
    }
  };

  const editFeedCategory = (feed: Feed) => {
    const value = window.prompt(
      "カテゴリ名を入力してください（空欄で未分類）",
      feed.category ?? ""
    );
    if (value == null) return;
    void moveFeed(feed.id, value.trim() || null);
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
          testId="all-articles"
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
        <SidebarRow
          label={summaryStats.pending > 0 ? `✦ AI要約（${summaryStats.pending}件生成中）` : "✦ AI要約"}
          count={summaryStats.unreviewed}
          error={summaryStats.failed > 0 ? `${summaryStats.failed}件の生成に失敗しました` : null}
          selected={isSelected({ kind: "summaries" })}
          testId="summary-inbox"
          onClick={() => onSelect({ kind: "summaries" })}
        />
        <SidebarRow
          label="✦ 今日のブリーフィング"
          selected={false}
          testId="open-briefing"
          onClick={onOpenBriefing}
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

      <div
        className={`section-label feed-drop-target ${dragTarget === "__uncategorized__" ? "drag-over" : ""}`}
        data-testid="uncategorized-drop-target"
        onDragOver={(event) => {
          event.preventDefault();
          setDragTarget("__uncategorized__");
        }}
        onDragLeave={() => setDragTarget(null)}
        onDrop={(event) => {
          event.preventDefault();
          const feedId = Number(event.dataTransfer.getData("text/feed-id"));
          if (feedId) void moveFeed(feedId, null);
        }}
      >
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
          <input
            list="feed-category-options"
            placeholder="カテゴリ（任意）"
            value={newFeedCategory}
            onChange={(e) => setNewFeedCategory(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
              if (e.key === "Escape") setAdding(false);
            }}
            disabled={busy}
          />
          <datalist id="feed-category-options">
            {categories.map((category) => (
              <option key={category} value={category} />
            ))}
          </datalist>
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
            onEditCategory={editFeedCategory}
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
                } ${dragTarget === category ? "drag-over" : ""}`}
                data-testid={`category-${category}`}
                onClick={() => onSelect({ kind: "category", category })}
                onDragOver={(event) => {
                  event.preventDefault();
                  setDragTarget(category);
                }}
                onDragLeave={() => setDragTarget(null)}
                onDrop={(event) => {
                  event.preventDefault();
                  const feedId = Number(event.dataTransfer.getData("text/feed-id"));
                  if (feedId) void moveFeed(feedId, category);
                }}
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
                <button
                  className="icon-button category-edit"
                  data-testid={`rename-category-${category}`}
                  title="カテゴリ名を変更"
                  onClick={(event) => {
                    event.stopPropagation();
                    const next = window.prompt("新しいカテゴリ名", category)?.trim();
                    if (next && next !== category) {
                      onRenameCategory(category, next).catch((e) =>
                        setImportNote(`カテゴリ名変更失敗: ${e}`)
                      );
                    }
                  }}
                >
                  ✎
                </button>
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
                    onEditCategory={editFeedCategory}
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
  onEditCategory,
}: {
  feed: Feed;
  indent?: boolean;
  isSelected: (sel: Selection) => boolean;
  onSelect: (sel: Selection) => void;
  onRemoveFeed: (feedId: number) => void;
  onEditCategory: (feed: Feed) => void;
}) {
  return (
    <div
      className={indent ? "feed-row-indent" : undefined}
      draggable
      onDragStart={(event) => {
        event.dataTransfer.effectAllowed = "move";
        event.dataTransfer.setData("text/feed-id", String(feed.id));
      }}
    >
      <SidebarRow
        label={feed.title || feed.url}
        count={feed.unread_count}
        error={feed.last_error}
        testId={`feed-${feed.id}`}
        selected={isSelected({ kind: "feed", feedId: feed.id })}
        onClick={() => onSelect({ kind: "feed", feedId: feed.id })}
        onCategorize={() => onEditCategory(feed)}
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
  onCategorize,
  testId,
}: {
  label: string;
  count?: number;
  error?: string | null;
  selected: boolean;
  onClick: () => void;
  onRemove?: () => void;
  onCategorize?: () => void;
  testId?: string;
}) {
  return (
    <div
      className={`sidebar-row ${selected ? "selected" : ""} ${error ? "has-error" : ""}`}
      data-testid={testId}
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
      {onCategorize && (
        <button
          className="icon-button row-action"
          data-testid={testId ? `${testId}-category` : undefined}
          title="カテゴリを変更"
          onClick={(e) => {
            e.stopPropagation();
            onCategorize();
          }}
        >
          ◫
        </button>
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
