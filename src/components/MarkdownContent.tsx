import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";

interface Props {
  text: string;
  className?: string;
}

/** Render generated Markdown without enabling embedded HTML. */
export function MarkdownContent({ text, className }: Props) {
  return (
    <div className={className}>
      <ReactMarkdown
        skipHtml
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(event) => {
                event.preventDefault();
                if (href && /^https?:\/\//i.test(href)) void openUrl(href);
              }}
            >
              {children}
            </a>
          ),
          // Generated Markdown must not trigger remote image requests.
          img: () => null,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
