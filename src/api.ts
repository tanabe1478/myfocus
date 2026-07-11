import { invoke } from "@tauri-apps/api/core";
import type { Article, Feed, Selection } from "./types";

export const listFeeds = () => invoke<Feed[]>("list_feeds");

export const listArticles = (sel: Selection) =>
  invoke<Article[]>("list_articles", {
    feedId: sel.kind === "feed" ? sel.feedId : null,
    unreadOnly: sel.kind === "unread",
    starredOnly: sel.kind === "starred",
  });

export const fuzzySearch = (query: string) =>
  invoke<Article[]>("fuzzy_search", { query });

export const addFeed = (url: string) => invoke<Feed>("add_feed", { url });

export const removeFeed = (feedId: number) => invoke<void>("remove_feed", { feedId });

export const markRead = (articleId: number, read: boolean) =>
  invoke<void>("mark_read", { articleId, read });

export const markStarred = (articleId: number, starred: boolean) =>
  invoke<void>("mark_starred", { articleId, starred });

export const markAllRead = (feedId: number | null) =>
  invoke<void>("mark_all_read", { feedId });

export const importOpml = (content: string) =>
  invoke<number>("import_opml", { content });

export const refreshAll = () =>
  invoke<{ new_articles: number; failed: string[] }>("refresh_all");

export const aiPrompt = (message: string) => invoke<void>("ai_prompt", { message });
export const aiAbort = () => invoke<void>("ai_abort");
export const aiNewSession = () => invoke<void>("ai_new_session");
