import { invoke } from "@tauri-apps/api/core";
import type { Article, Feed, HnItem, Selection } from "./types";

export const listFeeds = () => invoke<Feed[]>("list_feeds");

export const listArticles = (sel: Selection) =>
  invoke<Article[]>("list_articles", {
    feedId: sel.kind === "feed" ? sel.feedId : null,
    category: sel.kind === "category" ? sel.category : null,
    unreadOnly: sel.kind === "unread",
    starredOnly: sel.kind === "starred",
  });

export const fuzzySearch = (query: string) =>
  invoke<Article[]>("fuzzy_search", { query });

export const getArticle = (articleId: number) =>
  invoke<Article>("get_article", { articleId });

export const summarizeArticle = (articleId: number, force = false) =>
  invoke<Article>("summarize_article", { articleId, force });

export const getSetting = (key: string) =>
  invoke<string | null>("get_setting", { key });

export const setSetting = (key: string, value: string) =>
  invoke<void>("set_setting", { key, value });

export const openSettings = () => invoke<void>("open_settings");

export const addFeed = (url: string) => invoke<Feed>("add_feed", { url });

export const removeFeed = (feedId: number) => invoke<void>("remove_feed", { feedId });

export const markRead = (articleId: number, read: boolean) =>
  invoke<void>("mark_read", { articleId, read });

export const markStarred = (articleId: number, starred: boolean) =>
  invoke<void>("mark_starred", { articleId, starred });

export const markAllRead = (feedId: number | null, category: string | null = null) =>
  invoke<void>("mark_all_read", { feedId, category });

export const hnList = () => invoke<HnItem[]>("hn_list");

export const hnRefresh = () => invoke<number>("hn_refresh");

export const hnSummarizeComments = (itemId: number) =>
  invoke<string>("hn_summarize_comments", { itemId });

export const listPiModels = () => invoke<string[]>("list_pi_models");

export const importOpml = (content: string) =>
  invoke<number>("import_opml", { content });

export const openBackground = (url: string) =>
  invoke<void>("open_background", { url });

export const refreshAll = () =>
  invoke<{ new_articles: number; failed: string[] }>("refresh_all");

export const aiPrompt = (message: string) => invoke<void>("ai_prompt", { message });
export const aiAbort = () => invoke<void>("ai_abort");
export const aiNewSession = () => invoke<void>("ai_new_session");
