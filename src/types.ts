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

export type Selection =
  | { kind: "all" }
  | { kind: "unread" }
  | { kind: "starred" }
  | { kind: "feed"; feedId: number }
  | { kind: "category"; category: string };

export interface AiMessage {
  role: "user" | "assistant";
  text: string;
}
