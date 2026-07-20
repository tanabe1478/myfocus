import DOMPurify from "dompurify";
import { useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Article } from "../types";
import { relativeTime } from "../format";
import { openBackground } from "../api";
import { MarkdownContent } from "./MarkdownContent";

interface Props {
  article: Article | null;
  onToggleStar: (article: Article) => void;
  onToggleRead: (article: Article) => void;
  onSummarize: (article: Article, force: boolean) => Promise<Article>;
  onAskAi: (article: Article) => void;
  onFindRelated: (article: Article) => void;
}

export function ReadingPane({
  article,
  onToggleStar,
  onToggleRead,
  onSummarize,
  onAskAi,
  onFindRelated,
}: Props) {
  const [summarizing, setSummarizing] = useState(false);
  const [summaryError, setSummaryError] = useState<string | null>(null);

  useEffect(() => {
    setSummarizing(false);
    setSummaryError(null);
  }, [article?.id]);

  const generateSummary = async (force: boolean) => {
    if (!article || summarizing) return;
    setSummarizing(true);
    setSummaryError(null);
    try {
      await onSummarize(article, force);
    } catch (e) {
      setSummaryError(String(e));
    } finally {
      setSummarizing(false);
    }
  };

  const html = useMemo(() => {
    if (!article?.content_html) return null;
    const sanitized = DOMPurify.sanitize(article.content_html, {
      FORBID_TAGS: ["style", "form", "input"],
    });
    if (!article.url) return sanitized;

    // Feed HTML is injected into the Tauri document, so relative image/link
    // URLs would otherwise resolve against the app itself instead of the article.
    const doc = new DOMParser().parseFromString(sanitized, "text/html");
    const resolve = (value: string) => {
      try {
        return new URL(value, article.url!).toString();
      } catch {
        return value;
      }
    };
    doc.querySelectorAll<HTMLElement>("[src], [href], [poster]").forEach((el) => {
      for (const attr of ["src", "href", "poster"]) {
        const value = el.getAttribute(attr);
        if (value && !value.startsWith("data:") && !value.startsWith("blob:")) {
          el.setAttribute(attr, resolve(value));
        }
      }
    });
    doc.querySelectorAll<HTMLImageElement>("img[data-src]").forEach((img) => {
      if (!img.getAttribute("src")) img.src = resolve(img.dataset.src!);
    });
    doc.querySelectorAll<HTMLElement>("[srcset]").forEach((el) => {
      const srcset = el.getAttribute("srcset");
      if (!srcset) return;
      el.setAttribute(
        "srcset",
        srcset
          .split(",")
          .map((candidate) => {
            const [url, ...descriptor] = candidate.trim().split(/\s+/);
            return [resolve(url), ...descriptor].join(" ");
          })
          .join(", ")
      );
    });
    return doc.body.innerHTML;
  }, [article?.id, article?.content_html, article?.url]);

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
        <button
          className="icon-button"
          title="関連記事を探す"
          data-testid="find-related"
          onClick={() => onFindRelated(article)}
        >
          ≋
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
        <section className="article-ai-summary">
          {article.ai_summary ? (
            <>
              <div className="article-ai-summary-header">
                <span>✦ AI要約</span>
                <button onClick={() => generateSummary(true)} disabled={summarizing}>
                  {summarizing ? "再生成中…" : "再生成"}
                </button>
              </div>
              <MarkdownContent
                className="article-ai-summary-text markdown-content"
                text={article.ai_summary}
              />
              {article.ai_summary_model && (
                <div className="article-ai-summary-model">{article.ai_summary_model}</div>
              )}
            </>
          ) : (
            <button
              className="article-ai-summary-generate"
              onClick={() => generateSummary(false)}
              disabled={summarizing}
            >
              {summarizing ? "元記事を取得して要約しています…" : "✦ AI要約を生成"}
            </button>
          )}
          {summaryError && <div className="article-ai-summary-error">{summaryError}</div>}
        </section>
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
