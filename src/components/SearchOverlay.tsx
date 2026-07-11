import { useEffect, useRef, useState } from "react";
import type { Article } from "../types";
import { fuzzySearch } from "../api";
import { relativeTime } from "../format";

interface Props {
  onClose: () => void;
  onSelect: (article: Article) => void;
}

export function SearchOverlay({ onClose, onSelect }: Props) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Article[]>([]);
  const [cursor, setCursor] = useState(0);
  const seq = useRef(0);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const id = ++seq.current;
    if (!query.trim()) {
      setResults([]);
      return;
    }
    const timer = setTimeout(async () => {
      const found = await fuzzySearch(query);
      if (seq.current === id) {
        setResults(found);
        setCursor(0);
      }
    }, 120);
    return () => clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    listRef.current
      ?.querySelector(".search-result.selected")
      ?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => Math.min(c + 1, results.length - 1));
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => Math.max(c - 1, 0));
    }
    if (e.key === "Enter" && results[cursor]) {
      onSelect(results[cursor]);
    }
  };

  return (
    <div className="overlay-backdrop" onClick={onClose}>
      <div className="search-panel" onClick={(e) => e.stopPropagation()}>
        <input
          autoFocus
          className="search-input"
          placeholder="全文検索（スペース区切りでAND検索）"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="search-results" ref={listRef}>
          {results.map((a, i) => (
            <div
              key={a.id}
              className={`search-result ${i === cursor ? "selected" : ""}`}
              onMouseEnter={() => setCursor(i)}
              onClick={() => onSelect(a)}
            >
              <div className="search-result-title">{a.title || "(無題)"}</div>
              <div className="search-result-meta">
                {a.feed_title} · {relativeTime(a.published_at)}
              </div>
            </div>
          ))}
          {query.trim() && results.length === 0 && (
            <div className="empty-hint">一致する記事がありません</div>
          )}
        </div>
      </div>
    </div>
  );
}
