import DOMPurify from "dompurify";
import { useMemo } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Article } from "../types";
import { relativeTime } from "../format";
import { openBackground } from "../api";

interface Props {
  article: Article | null;
  onToggleStar: (article: Article) => void;
  onToggleRead: (article: Article) => void;
  onAskAi: (article: Article) => void;
}

export function ReadingPane({ article, onToggleStar, onToggleRead, onAskAi }: Props) {
  const html = useMemo(() => {
    if (!article?.content_html) return null;
    return DOMPurify.sanitize(article.content_html, {
      FORBID_TAGS: ["style", "form", "input"],
    });
  }, [article?.id, article?.content_html]);

  if (!article) {
    return (
      <section className="reading-pane">
        <div className="reading-empty">記事を選択してください</div>
      </section>
    );
  }

  return (
    <section className="reading-pane">
      <div className="pane-header reading-toolbar">
        <button
          className="icon-button"
          title={article.starred ? "スターを外す" : "スターを付ける"}
          onClick={() => onToggleStar(article)}
        >
          {article.starred ? "★" : "☆"}
        </button>
        <button
          className="icon-button"
          title={article.read ? "未読に戻す" : "既読にする"}
          onClick={() => onToggleRead(article)}
        >
          {article.read ? "◌" : "✓"}
        </button>
        <button className="icon-button" title="AIに相談" onClick={() => onAskAi(article)}>
          ✦
        </button>
        {article.url && (
          <>
            <button
              className="icon-button"
              title="ブラウザで開く"
              onClick={() => article.url && openUrl(article.url)}
            >
              ↗
            </button>
            <button
              className="icon-button"
              title="バックグラウンドでブラウザで開く（b）"
              onClick={() => article.url && openBackground(article.url)}
            >
              ⧉
            </button>
          </>
        )}
      </div>
      <article className="reading-body">
        <div className="reading-meta">
          {article.feed_title}
          {article.author ? ` · ${article.author}` : ""}
          {article.published_at ? ` · ${relativeTime(article.published_at)}` : ""}
        </div>
        <h1
          className="reading-title"
          onClick={() => article.url && openUrl(article.url)}
          style={{ cursor: article.url ? "pointer" : "default" }}
        >
          {article.title || "(無題)"}
        </h1>
        {html ? (
          <div
            className="reading-content"
            dangerouslySetInnerHTML={{ __html: html }}
            onClick={(e) => {
              // open links in the external browser instead of the webview
              const anchor = (e.target as HTMLElement).closest("a");
              if (anchor?.href) {
                e.preventDefault();
                openUrl(anchor.href);
              }
            }}
          />
        ) : (
          <div className="reading-content">
            <p>{article.summary ?? "本文がありません。タイトルをクリックするとブラウザで開きます。"}</p>
          </div>
        )}
      </article>
    </section>
  );
}
