import { invoke } from "@tauri-apps/api/core";
import type { Article, Digest, Feed, Selection } from "./types";

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

export const getSetting = (key: string) =>
  invoke<string | null>("get_setting", { key });

export const setSetting = (key: string, value: string) =>
  invoke<void>("set_setting", { key, value });

export const addFeed = (url: string) => invoke<Feed>("add_feed", { url });

export const removeFeed = (feedId: number) => invoke<void>("remove_feed", { feedId });

export const markRead = (articleId: number, read: boolean) =>
  invoke<void>("mark_read", { articleId, read });

export const markStarred = (articleId: number, starred: boolean) =>
  invoke<void>("mark_starred", { articleId, starred });

export const markAllRead = (feedId: number | null, category: string | null = null) =>
  invoke<void>("mark_all_read", { feedId, category });

export const setDigestRule = (feedId: number, enabled: boolean) =>
  invoke<void>("set_digest_rule", { feedId, enabled });

export const listDigestRules = () => invoke<number[]>("list_digest_rules");

export const getDigests = (articleIds: number[]) =>
  invoke<Record<number, Digest>>("get_digests", { articleIds });

export const summarizeComments = (articleId: number) =>
  invoke<string>("summarize_comments", { articleId });

export const importOpml = (content: string) =>
  invoke<number>("import_opml", { content });

export const openBackground = (url: string) =>
  invoke<void>("open_background", { url });

export const refreshAll = () =>
  invoke<{ new_articles: number; failed: string[] }>("refresh_all");

export const aiPrompt = (message: string) => invoke<void>("ai_prompt", { message });
export const aiAbort = () => invoke<void>("ai_abort");
export const aiNewSession = () => invoke<void>("ai_new_session");
