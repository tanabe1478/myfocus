import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { Article, Feed, Selection } from "./types";
import * as api from "./api";
import { htmlToText } from "./format";
import { usePi } from "./usePi";
import { Sidebar } from "./components/Sidebar";
import { ArticleList } from "./components/ArticleList";
import { ReadingPane } from "./components/ReadingPane";
import { SearchOverlay } from "./components/SearchOverlay";
import { AiPanel } from "./components/AiPanel";
import "./App.css";

export default function App() {
  const [feeds, setFeeds] = useState<Feed[]>([]);
  const [selection, setSelection] = useState<Selection>({ kind: "unread" });
  const [articles, setArticles] = useState<Article[]>([]);
  const [selected, setSelected] = useState<Article | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [aiOpen, setAiOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [translating, setTranslating] = useState({ active: false, remaining: 0 });

  // feeds-updated ハンドラから最新の選択記事を参照するための ref
  const selectedRef = useRef<Article | null>(null);
  selectedRef.current = selected;

  const pi = usePi();

  const reloadFeeds = useCallback(async () => {
    setFeeds(await api.listFeeds());
  }, []);

  const reloadArticles = useCallback(async () => {
    setArticles(await api.listArticles(selection));
  }, [selection]);

  useEffect(() => {
    reloadFeeds();
  }, [reloadFeeds]);

  useEffect(() => {
    reloadArticles();
  }, [reloadArticles]);

  useEffect(() => {
    const unlisten = listen("feeds-updated", () => {
      reloadFeeds();
      reloadArticles();
      // 開いている記事に翻訳・要約が届いたら差し替える（既読/スターの楽観的更新は保持）
      const cur = selectedRef.current;
      if (cur) {
        api
          .getArticle(cur.id)
          .then((full) =>
            setSelected((c) =>
              c?.id === cur.id ? { ...full, read: c.read, starred: c.starred } : c
            )
          )
          .catch(() => {});
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [reloadFeeds, reloadArticles]);

  useEffect(() => {
    const unlisten = listen<string>("translate-error", (e) => {
      setToast(`翻訳エラー: ${e.payload}`);
      setTimeout(() => setToast(null), 8000);
    });
    const unlistenStatus = listen<{ active: boolean; remaining: number }>(
      "translate-status",
      (e) => setTranslating(e.payload)
    );
    return () => {
      unlisten.then((f) => f());
      unlistenStatus.then((f) => f());
    };
  }, []);

  const translateFeedIds = useMemo(
    () => new Set(feeds.filter((f) => f.translate).map((f) => f.id)),
    [feeds]
  );

  const selectArticle = useCallback((article: Article) => {
    setSelected(article);
    // list rows omit content_html; load the full article for the reading pane
    api
      .getArticle(article.id)
      .then((full) =>
        setSelected((cur) => (cur?.id === article.id ? { ...full, read: true } : cur))
      )
      .catch(() => {});
    if (!article.read) {
      api.markRead(article.id, true);
      setArticles((list) =>
        list.map((a) => (a.id === article.id ? { ...a, read: true } : a))
      );
      setFeeds((list) =>
        list.map((f) =>
          f.id === article.feed_id
            ? { ...f, unread_count: Math.max(0, f.unread_count - 1) }
            : f
        )
      );
    }
  }, []);

  // global shortcuts: ⌘K search, j/k article navigation
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setSearchOpen((v) => !v);
        return;
      }
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      if (e.key === "b") {
        // 選択中の記事をバックグラウンドでブラウザに開く（アプリは最前面のまま）
        e.preventDefault();
        setSelected((cur) => {
          if (cur?.url) api.openBackground(cur.url).catch(() => {});
          return cur;
        });
        return;
      }
      if (e.key === "j" || e.key === "k") {
        e.preventDefault();
        setSelected((cur) => {
          const idx = cur ? articles.findIndex((a) => a.id === cur.id) : -1;
          const next = articles[e.key === "j" ? idx + 1 : Math.max(idx - 1, 0)];
          if (next && next.id !== cur?.id) {
            selectArticle(next);
            return next;
          }
          return cur;
        });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [articles, selectArticle]);

  const handleAddFeed = useCallback(
    async (url: string) => {
      await api.addFeed(url);
      await reloadFeeds();
    },
    [reloadFeeds]
  );

  const handleRemoveFeed = useCallback(
    async (feedId: number) => {
      await api.removeFeed(feedId);
      if (selection.kind === "feed" && selection.feedId === feedId) {
        setSelection({ kind: "unread" });
      }
      setSelected((cur) => (cur?.feed_id === feedId ? null : cur));
      await reloadFeeds();
      await reloadArticles();
    },
    [selection, reloadFeeds, reloadArticles]
  );

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await api.refreshAll();
    } finally {
      setRefreshing(false);
    }
  }, []);

  const handleMarkAllRead = useCallback(async () => {
    await api.markAllRead(
      selection.kind === "feed" ? selection.feedId : null,
      selection.kind === "category" ? selection.category : null
    );
    await reloadFeeds();
    await reloadArticles();
  }, [selection, reloadFeeds, reloadArticles]);

  const toggleStar = useCallback((article: Article) => {
    const starred = !article.starred;
    api.markStarred(article.id, starred);
    setSelected((cur) => (cur?.id === article.id ? { ...cur, starred } : cur));
    setArticles((list) => list.map((a) => (a.id === article.id ? { ...a, starred } : a)));
  }, []);

  const toggleRead = useCallback((article: Article) => {
    const read = !article.read;
    api.markRead(article.id, read);
    setSelected((cur) => (cur?.id === article.id ? { ...cur, read } : cur));
    setArticles((list) => list.map((a) => (a.id === article.id ? { ...a, read } : a)));
    setFeeds((list) =>
      list.map((f) =>
        f.id === article.feed_id
          ? { ...f, unread_count: Math.max(0, f.unread_count + (read ? -1 : 1)) }
          : f
      )
    );
  }, []);

  const askAiAboutArticle = useCallback(
    (article: Article) => {
      setAiOpen(true);
      const body = article.content_html
        ? htmlToText(article.content_html)
        : article.summary ?? "";
      pi.send(
        `次の記事について相談させてください。まず要点を3行で要約してください。\n\n` +
          `タイトル: ${article.title}\nURL: ${article.url ?? "不明"}\n\n本文:\n${body}`
      );
    },
    [pi.send]
  );

  const handleToggleTranslate = useCallback(
    async (feed: Feed) => {
      const next = !feed.translate;
      await api.setFeedTranslate(feed.id, next);
      setFeeds((list) =>
        list.map((f) => (f.id === feed.id ? { ...f, translate: next } : f))
      );
      if (next) {
        // 本文取得中でステータスイベントが届く前から進行中表示にする
        setTranslating((t) => (t.active ? t : { active: true, remaining: feed.unread_count }));
      }
    },
    []
  );

  const handleSubscribeSuggestion = useCallback(
    async (url: string) => {
      try {
        await handleAddFeed(url);
      } catch (e) {
        alert(`購読に失敗しました: ${e}`);
      }
    },
    [handleAddFeed]
  );

  const totalUnread = useMemo(
    () => feeds.reduce((sum, f) => sum + f.unread_count, 0),
    [feeds]
  );

  const listTitle = useMemo(() => {
    switch (selection.kind) {
      case "all":
        return "すべて";
      case "unread":
        return "未読";
      case "starred":
        return "スター付き";
      case "feed":
        return feeds.find((f) => f.id === selection.feedId)?.title ?? "フィード";
      case "category":
        return selection.category;
    }
  }, [selection, feeds]);

  return (
    <div className="app">
      <Sidebar
        feeds={feeds}
        selection={selection}
        totalUnread={totalUnread}
        refreshing={refreshing}
        onSelect={setSelection}
        onAddFeed={handleAddFeed}
        onRemoveFeed={handleRemoveFeed}
        onRefresh={handleRefresh}
        onImportOpml={api.importOpml}
        onToggleTranslate={handleToggleTranslate}
        translating={translating}
      />
      <ArticleList
        articles={articles}
        selectedId={selected?.id ?? null}
        title={listTitle}
        translatingFeedIds={translateFeedIds}
        onSelect={selectArticle}
        onMarkAllRead={handleMarkAllRead}
      />
      <ReadingPane
        article={selected}
        digestPending={
          !!selected &&
          !selected.summary_ja &&
          !selected.title_ja &&
          translateFeedIds.has(selected.feed_id)
        }
        onToggleStar={toggleStar}
        onToggleRead={toggleRead}
        onAskAi={askAiAboutArticle}
      />
      {aiOpen ? (
        <AiPanel
          messages={pi.messages}
          busy={pi.busy}
          status={pi.status}
          onSend={pi.send}
          onAbort={pi.abort}
          onReset={pi.reset}
          onSubscribe={handleSubscribeSuggestion}
          onClose={() => setAiOpen(false)}
        />
      ) : (
        <button className="ai-fab" title="AIアシスタント" onClick={() => setAiOpen(true)}>
          ✦
        </button>
      )}
      {toast && <div className="toast">{toast}</div>}
      {searchOpen && (
        <SearchOverlay
          onClose={() => setSearchOpen(false)}
          onSelect={(a) => {
            setSearchOpen(false);
            setSelection({ kind: "all" });
            selectArticle(a);
          }}
        />
      )}
    </div>
  );
}
