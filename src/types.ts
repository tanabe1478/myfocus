export interface Feed {
  id: number;
  url: string;
  title: string;
  site_url: string | null;
  category: string | null;
  last_fetched_at: number | null;
  last_error: string | null;
  unread_count: number;
}

export interface Article {
  id: number;
  feed_id: number;
  feed_title: string;
  guid: string;
  title: string;
  url: string | null;
  author: string | null;
  summary: string | null;
  content_html: string | null;
  published_at: number | null;
  read: boolean;
  starred: boolean;
}

/** アイテムに添付される日本語ダイジェスト（翻訳エンジンの生成物） */
export interface Digest {
  title_ja: string | null;
  summary_ja: string | null;
  comments_summary_ja: string | null;
}

/** Hacker Newsモジュールのアイテム */
export interface HnItem {
  id: number;
  title: string;
  url: string | null;
  comments_url: string;
  points: number;
  comments_count: number;
  rank: number;
  digest: Digest | null;
}

export type Selection =
  | { kind: "all" }
  | { kind: "unread" }
  | { kind: "starred" }
  | { kind: "feed"; feedId: number }
  | { kind: "category"; category: string }
  | { kind: "hn" };

export interface AiMessage {
  role: "user" | "assistant";
  text: string;
}
