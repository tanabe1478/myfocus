import { useEffect, useMemo, useRef, useState } from "react";
import type { Feed, Selection } from "../types";
import { getSetting, listPiModels, setSetting } from "../api";

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
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);
  const [settingsOpen, setSettingsOpen] = useState(false);
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
            title="設定"
            onClick={() => setSettingsOpen((v) => !v)}
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

      {settingsOpen && <SettingsPanel onClose={() => setSettingsOpen(false)} />}

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

const DEFAULT_TRANSLATE_MODEL = "openai-codex/gpt-5.6-luna";

function SettingsPanel({ onClose }: { onClose: () => void }) {
  const [days, setDays] = useState("");
  const [model, setModel] = useState(DEFAULT_TRANSLATE_MODEL);
  const [models, setModels] = useState<string[]>([]);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getSetting("retention_days").then((v) => setDays(v ?? "90"));
    getSetting("translate_model").then((v) => setModel(v || DEFAULT_TRANSLATE_MODEL));
    listPiModels().then(setModels).catch(() => setModels([]));
  }, []);

  const save = async () => {
    const n = parseInt(days, 10);
    if (isNaN(n) || n < 0) {
      setNote("0以上の日数を入力してください");
      return;
    }
    await setSetting("retention_days", String(n));
    await setSetting("translate_model", model);
    setNote("保存しました");
    setTimeout(onClose, 1200);
  };

  return (
    <div className="settings-panel">
      <div className="settings-title">記事の保持期間</div>
      <div className="settings-row">
        <input
          type="number"
          min={0}
          value={days}
          onChange={(e) => setDays(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") onClose();
          }}
        />
        <span>日</span>
        <button className="settings-save" onClick={save}>
          保存
        </button>
      </div>
      <div className="settings-hint">
        既読記事をこの日数の経過後に自動削除します。スター付きは削除されません。0で無効。
      </div>

      <div className="settings-title" style={{ marginTop: 12 }}>
        翻訳・要約のモデル
      </div>
      <div className="settings-row">
        <select
          className="settings-model-input"
          value={model}
          onChange={(e) => setModel(e.target.value)}
        >
          {models.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
          {!models.includes(model) && <option value={model}>{model}</option>}
        </select>
        <button className="settings-save" onClick={save}>
          保存
        </button>
      </div>
      <div className="settings-hint">
        Hacker Newsの新しい翻訳・要約から適用されます。既存の生成結果は保持されます。
      </div>
      {note && <div className="add-feed-status">{note}</div>}
    </div>
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
