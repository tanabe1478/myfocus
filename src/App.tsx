import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { Article, Feed, SavedSearch, Selection } from "./types";
import * as api from "./api";
import { htmlToText } from "./format";
import { usePi } from "./usePi";
import {
  DEFAULT_SHORTCUTS,
  matchesShortcut,
  parseShortcuts,
  type KeyboardShortcuts,
} from "./shortcuts";
import { Sidebar } from "./components/Sidebar";
import { ArticleList } from "./components/ArticleList";
import { ReadingPane } from "./components/ReadingPane";
import { SearchOverlay } from "./components/SearchOverlay";
import { AiPanel } from "./components/AiPanel";
import { HackerNewsPane } from "./components/HackerNewsPane";
import "./App.css";

export default function App() {
  const [feeds, setFeeds] = useState<Feed[]>([]);
  const [selection, setSelection] = useState<Selection>({ kind: "unread" });
  const [articles, setArticles] = useState<Article[]>([]);
  const [selected, setSelected] = useState<Article | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [aiOpen, setAiOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [shortcuts, setShortcuts] = useState<KeyboardShortcuts>(DEFAULT_SHORTCUTS);
  const [savedSearches, setSavedSearches] = useState<SavedSearch[]>([]);

  // feeds-updated ハンドラから最新の選択記事を参照するための ref
  const selectedRef = useRef<Article | null>(null);
  selectedRef.current = selected;

  const pi = usePi();

  // Match the source order rendered by Sidebar: uncategorized feeds first,
  // then each category row followed by the feeds it contains.
  const feedNavigation = useMemo<Selection[]>(() => {
    const uncategorized: Feed[] = [];
    const categories = new Map<string, Feed[]>();
    for (const feed of feeds) {
      if (!feed.category) {
        uncategorized.push(feed);
        continue;
      }
      const group = categories.get(feed.category) ?? [];
      group.push(feed);
      categories.set(feed.category, group);
    }
    return [
      ...uncategorized.map((feed) => ({ kind: "feed", feedId: feed.id }) as Selection),
      ...[...categories.entries()].flatMap(([category, group]) => [
        { kind: "category", category } as Selection,
        ...group.map((feed) => ({ kind: "feed", feedId: feed.id }) as Selection),
      ]),
    ];
  }, [feeds]);

  useEffect(() => {
    const reloadShortcuts = () =>
      api.getSetting("keyboard_shortcuts").then((v) => setShortcuts(parseShortcuts(v)));
    const reloadSavedSearches = () =>
      api.getSetting("saved_searches").then((v) => {
        try {
          setSavedSearches(v ? (JSON.parse(v) as SavedSearch[]) : []);
        } catch {
          setSavedSearches([]);
        }
      });
    reloadShortcuts();
    reloadSavedSearches();
    const unlisten = listen<string>("settings-updated", (e) => {
      if (e.payload === "keyboard_shortcuts") reloadShortcuts();
      if (e.payload === "saved_searches") reloadSavedSearches();
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const reloadFeeds = useCallback(async () => {
    setFeeds(await api.listFeeds());
  }, []);

  const reloadArticles = useCallback(async () => {
    if (selection.kind === "hn") return;
    const next =
      selection.kind === "search"
        ? await api.fuzzySearch(selection.query)
        : await api.listArticles(selection);
    setArticles(next);
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

  const handleSaveSearch = useCallback((query: string) => {
    const trimmed = query.trim();
    if (!trimmed) return;
    setSavedSearches((current) => {
      if (current.some((search) => search.query.toLowerCase() === trimmed.toLowerCase())) {
        return current;
      }
      const next = [
        ...current,
        { id: crypto.randomUUID(), name: trimmed, query: trimmed },
      ];
      api.setSetting("saved_searches", JSON.stringify(next));
      return next;
    });
  }, []);

  const handleRemoveSavedSearch = useCallback(
    (searchId: string) => {
      setSavedSearches((current) => {
        const next = current.filter((search) => search.id !== searchId);
        api.setSetting("saved_searches", JSON.stringify(next));
        return next;
      });
      if (selection.kind === "search" && selection.searchId === searchId) {
        setSelection({ kind: "unread" });
        setSelected(null);
      }
    },
    [selection]
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
    if (selection.kind === "hn" || selection.kind === "search") return;
    await api.markAllRead(
      selection.kind === "feed" ? selection.feedId : null,
      selection.kind === "category" ? selection.category : null
    );
    await reloadFeeds();
    await reloadArticles();
  }, [selection, reloadFeeds, reloadArticles]);

  // Configurable global shortcuts. Search/settings also work while an input is focused.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (matchesShortcut(e, shortcuts.openSettings)) {
        e.preventDefault();
        api.openSettings();
        return;
      }
      if (matchesShortcut(e, shortcuts.search)) {
        e.preventDefault();
        setSearchOpen((v) => !v);
        return;
      }
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

      const feedDirection = matchesShortcut(e, shortcuts.nextFeed)
        ? 1
        : matchesShortcut(e, shortcuts.previousFeed)
          ? -1
          : 0;
      if (feedDirection) {
        e.preventDefault();
        const currentIndex = feedNavigation.findIndex(
          (item) => JSON.stringify(item) === JSON.stringify(selection)
        );
        const baseIndex =
          currentIndex >= 0 ? currentIndex : feedDirection > 0 ? -1 : feedNavigation.length;
        const nextSelection = feedNavigation[baseIndex + feedDirection];
        if (nextSelection) {
          setSelection(nextSelection);
          setSelected(null);
          // Do not retain the previous feed's scroll/focus while the new list loads.
          setArticles([]);
        }
        return;
      }
      if (
        matchesShortcut(e, shortcuts.markAllRead) &&
        selection.kind !== "hn" &&
        selection.kind !== "search"
      ) {
        e.preventDefault();
        if (confirm("現在の表示範囲の記事をすべて既読にしますか？")) {
          handleMarkAllRead();
        }
        return;
      }
      if (matchesShortcut(e, shortcuts.openBackground)) {
        e.preventDefault();
        setSelected((cur) => {
          if (cur?.url) api.openBackground(cur.url).catch(() => {});
          return cur;
        });
        return;
      }
      const direction = matchesShortcut(e, shortcuts.nextArticle)
        ? 1
        : matchesShortcut(e, shortcuts.previousArticle)
          ? -1
          : 0;
      if (direction) {
        e.preventDefault();
        setSelected((cur) => {
          const idx = cur ? articles.findIndex((a) => a.id === cur.id) : -1;
          const next = articles[direction > 0 ? idx + 1 : Math.max(idx - 1, 0)];
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
  }, [articles, feedNavigation, handleMarkAllRead, selectArticle, selection, shortcuts]);

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

  const summarizeArticle = useCallback(async (article: Article, force: boolean) => {
    const updated = await api.summarizeArticle(article.id, force);
    setSelected((current) => (current?.id === updated.id ? updated : current));
    return updated;
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

  const handleOpenAiArticle = useCallback(
    async (articleId: number) => {
      try {
        const article = await api.getArticle(articleId);
        setSelection({ kind: "all" });
        selectArticle(article);
      } catch (e) {
        alert(`記事を開けませんでした: ${e}`);
      }
    },
    [selectArticle]
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
      case "search":
        return `検索: ${selection.name}`;
      case "hn":
        return "Hacker News";
    }
  }, [selection, feeds]);

  return (
    <div className="app">
      <Sidebar
        feeds={feeds}
        selection={selection}
        savedSearches={savedSearches}
        totalUnread={totalUnread}
        refreshing={refreshing}
        onSelect={(next) => {
          setSelection(next);
          setSelected(null);
        }}
        onRemoveSavedSearch={handleRemoveSavedSearch}
        onAddFeed={handleAddFeed}
        onRemoveFeed={handleRemoveFeed}
        onRefresh={handleRefresh}
        onImportOpml={api.importOpml}
      />
      {selection.kind === "hn" ? (
        <HackerNewsPane />
      ) : (
        <>
          <ArticleList
            key={JSON.stringify(selection)}
            articles={articles}
            selectedId={selected?.id ?? null}
            title={listTitle}
            onSelect={selectArticle}
            onMarkAllRead={selection.kind === "search" ? undefined : handleMarkAllRead}
          />
          <ReadingPane
            article={selected}
            onToggleStar={toggleStar}
            onToggleRead={toggleRead}
            onSummarize={summarizeArticle}
            onAskAi={askAiAboutArticle}
          />
        </>
      )}
      {aiOpen ? (
        <AiPanel
          messages={pi.messages}
          busy={pi.busy}
          status={pi.status}
          onSend={pi.send}
          onAbort={pi.abort}
          onReset={pi.reset}
          onSubscribe={handleSubscribeSuggestion}
          onOpenArticle={handleOpenAiArticle}
          onClose={() => setAiOpen(false)}
        />
      ) : (
        <button className="ai-fab" title="AIアシスタント" onClick={() => setAiOpen(true)}>
          ✦
        </button>
      )}
      {searchOpen && (
        <SearchOverlay
          savedQueries={savedSearches.map((search) => search.query)}
          onSave={handleSaveSearch}
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
