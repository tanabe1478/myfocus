import { useRef, useState } from "react";
import type { Feed, Selection } from "../types";

interface Props {
  feeds: Feed[];
  selection: Selection;
  totalUnread: number;
  refreshing: boolean;
  onSelect: (sel: Selection) => void;
  onAddFeed: (url: string) => Promise<void>;
  onRemoveFeed: (feedId: number) => void;
  onRefresh: () => void;
  onImportOpml: (content: string) => Promise<number>;
}

export function Sidebar({
  feeds,
  selection,
  totalUnread,
  refreshing,
  onSelect,
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
  const fileRef = useRef<HTMLInputElement>(null);

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
        <button
          className="icon-button"
          title="すべて更新"
          onClick={onRefresh}
          disabled={refreshing}
        >
          {refreshing ? "…" : "⟳"}
        </button>
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
        {feeds.map((f) => (
          <SidebarRow
            key={f.id}
            label={f.title || f.url}
            count={f.unread_count}
            selected={isSelected({ kind: "feed", feedId: f.id })}
            onClick={() => onSelect({ kind: "feed", feedId: f.id })}
            onRemove={() => {
              if (confirm(`「${f.title}」の購読を解除しますか？記事も削除されます。`)) {
                onRemoveFeed(f.id);
              }
            }}
          />
        ))}
        {feeds.length === 0 && (
          <div className="empty-hint">＋ からフィードを追加してください</div>
        )}
      </div>
    </aside>
  );
}

function SidebarRow({
  label,
  count,
  selected,
  onClick,
  onRemove,
}: {
  label: string;
  count?: number;
  selected: boolean;
  onClick: () => void;
  onRemove?: () => void;
}) {
  return (
    <div className={`sidebar-row ${selected ? "selected" : ""}`} onClick={onClick}>
      <span className="sidebar-row-label" title={label}>
        {label}
      </span>
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
