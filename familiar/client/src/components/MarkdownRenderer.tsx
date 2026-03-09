import { useMemo } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";

interface Props {
  content: string;
  className?: string;
}

// Configure marked once
marked.setOptions({
  breaks: true,
  gfm: true,
});

export function MarkdownRenderer({ content, className }: Props) {
  const html = useMemo(() => {
    const raw = marked.parse(content) as string;
    return DOMPurify.sanitize(raw, {
      ALLOWED_TAGS: [
        "p", "br", "strong", "em", "del", "code", "pre",
        "h1", "h2", "h3", "h4", "h5", "h6",
        "ul", "ol", "li",
        "blockquote", "hr",
        "a", "img",
        "table", "thead", "tbody", "tr", "th", "td",
        "span", "div",
      ],
      ALLOWED_ATTR: ["href", "src", "alt", "title", "class", "target", "rel"],
      FORCE_BODY: false,
      RETURN_DOM_FRAGMENT: false,
      ADD_ATTR: ["target"],
    });
  }, [content]);

  return (
    <div
      className={`prose${className ? ` ${className}` : ""}`}
      // eslint-disable-next-line react/no-danger
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
