import DOMPurify from "dompurify";
import { useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Article } from "../types";
import { relativeTime } from "../format";
import { openBackground, summarizeComments } from "../api";

interface Props {
  article: Article | null;
  /** この記事のダイジェストが現在バックグラウンド生成中 */
  digestPending: boolean;
  onToggleStar: (article: Article) => void;
  onToggleRead: (article: Article) => void;
  onAskAi: (article: Article) => void;
}

export function ReadingPane({ article, digestPending, onToggleStar, onToggleRead, onAskAi }: Props) {
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
          {article.title_ja || article.title || "(無題)"}
        </h1>
        {article.title_ja && article.title_ja !== article.title && (
          <div className="reading-original-title">{article.title}</div>
        )}

        {article.summary_ja ? (
          <div className="digest-box">
            <div className="digest-label">AIダイジェスト</div>
            {article.summary_ja.split("\n\n").map((p, i) => (
              <p key={i}>{p}</p>
            ))}
          </div>
        ) : digestPending ? (
          <div className="digest-box digest-pending">
            <div className="digest-label">AIダイジェスト</div>
            <p>翻訳キューに入っています。本文を読んでダイジェストを生成します…</p>
          </div>
        ) : null}

        <CommentsSummary key={article.id} article={article} />

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

/// hackernews-ja風の「コメントの要約を表示」。初回クリックで生成し、以後はDBキャッシュ。
function CommentsSummary({ article }: { article: Article }) {
  const [summary, setSummary] = useState<string | null>(article.comments_summary_ja);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(!!article.comments_summary_ja);

  useEffect(() => {
    setSummary(article.comments_summary_ja);
    setOpen(!!article.comments_summary_ja);
    setError(null);
  }, [article.id]);

  if (!article.comments_url && !article.url) return null;

  const load = async () => {
    if (summary) {
      setOpen((v) => !v);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const result = await summarizeComments(article.id);
      setSummary(result);
      setOpen(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="comments-summary">
      <button className="comments-summary-toggle" onClick={load} disabled={loading}>
        {loading
          ? "コメントを読んで要約しています…"
          : summary
            ? open
              ? "コメントの要約を隠す"
              : "コメントの要約を表示"
            : "コメントの要約を生成"}
      </button>
      {error && <div className="add-feed-error">{error}</div>}
      {open && summary && (
        <div className="digest-box comments-box">
          <div className="digest-label">
            コメント要約
            {article.comments_url && (
              <a
                href={article.comments_url}
                onClick={(e) => {
                  e.preventDefault();
                  openUrl(article.comments_url!);
                }}
              >
                スレッドを開く ↗
              </a>
            )}
          </div>
          {summary.split("\n").map((line, i) => (
            <p key={i} className="comments-line">
              {line}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
