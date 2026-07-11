import { useCallback, useEffect, useMemo, useState } from "react";
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
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [reloadFeeds, reloadArticles]);

  const selectArticle = useCallback((article: Article) => {
    setSelected(article);
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
    await api.markAllRead(selection.kind === "feed" ? selection.feedId : null);
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
      />
      <ArticleList
        articles={articles}
        selectedId={selected?.id ?? null}
        title={listTitle}
        onSelect={selectArticle}
        onMarkAllRead={handleMarkAllRead}
      />
      <ReadingPane
        article={selected}
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
