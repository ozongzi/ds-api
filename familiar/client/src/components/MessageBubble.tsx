import { memo, useState, useCallback, useEffect, useRef } from "react";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { DiffView } from "./DiffView";
import { TerminalView } from "./TerminalView";
import type { ChatBubble } from "../api/types";
import styles from "./MessageBubble.module.css";

interface Props {
  bubble: ChatBubble;
}

export const MessageBubble = memo(function MessageBubble({ bubble }: Props) {
  if (bubble.kind === "tool") {
    return <ToolCallBubble bubble={bubble} />;
  }
  return <TextChatBubble bubble={bubble} />;
});

// ─── Text bubble (user / assistant) ──────────────────────────────────────────

function TextChatBubble({
  bubble,
}: {
  bubble: Extract<ChatBubble, { kind: "text" }>;
}) {
  const isUser = bubble.role === "user";

  return (
    <div
      className={`${styles.row} ${isUser ? styles.rowUser : styles.rowAssistant}`}
    >
      {!isUser && (
        <div className={styles.avatar} aria-hidden="true">
          🐱
        </div>
      )}

      <div
        className={`${styles.bubble} ${isUser ? styles.bubbleUser : styles.bubbleAssistant}`}
      >
        {isUser ? (
          <p className={styles.userText}>{bubble.content}</p>
        ) : (
          <>
            <MarkdownRenderer content={bubble.content} />
            {bubble.streaming && bubble.content.length === 0 && (
              <span className={styles.typingDots} aria-label="正在输入">
                <span />
                <span />
                <span />
              </span>
            )}
            {bubble.streaming && bubble.content.length > 0 && (
              <span className={styles.cursor} aria-hidden="true" />
            )}
          </>
        )}
      </div>

      {isUser && (
        <div
          className={`${styles.avatar} ${styles.avatarUser}`}
          aria-hidden="true"
        >
          你
        </div>
      )}
    </div>
  );
}

// ─── Tool call bubble ─────────────────────────────────────────────────────────

function ToolCallBubble({
  bubble,
}: {
  bubble: Extract<ChatBubble, { kind: "tool" }>;
}) {
  const [expanded, setExpanded] = useState(false);
  const autoExpandedRef = useRef(false);

  // Auto-expand the first time args start streaming in.
  if (!autoExpandedRef.current && bubble.argsRaw.length > 0 && !expanded) {
    autoExpandedRef.current = true;
    setExpanded(true);
  }

  // Detect present_file result
  const fileResult =
    bubble.result &&
    typeof bubble.result === "object" &&
    (bubble.result as Record<string, unknown>)["display"] === "file"
      ? (bubble.result as {
          display: "file";
          filename: string;
          path: string;
          size: number;
        })
      : null;

  // If this is a present_file tool call, render as a file card
  if (bubble.name === "present_file" && fileResult) {
    return <FileCard file={fileResult} pending={bubble.pending} />;
  }

  // ── Route to specialised views based on tool name ─────────────────────────

  const args = bubble.args as Record<string, unknown> | null;
  const result = bubble.result as Record<string, unknown> | null;

  // Terminal tools: execute, run_ts, run_py
  const isTerminal =
    bubble.name === "execute" ||
    bubble.name === "run_ts" ||
    bubble.name === "run_py";

  // Diff tools: str_replace (has old_str + new_str), write (has path + content)
  const isDiff =
    !bubble.pending &&
    result?.status === "success" &&
    ((bubble.name === "str_replace" &&
      args?.old_str !== undefined &&
      args?.new_str !== undefined) ||
      (bubble.name === "write" &&
        args?.path !== undefined &&
        args?.content !== undefined));

  // ── Tool header (shared across all variants) ──────────────────────────────

  const toolIcon = bubble.pending ? "⚙️" : "✅";
  const toolLabel = isTerminal
    ? args?.command
      ? String(args.command)
      : bubble.name
    : bubble.name;

  // For terminal/diff tools we show the specialised view inline (no expand).
  // For other tools we keep the collapsible generic view.
  const isInline = isTerminal || isDiff;

  // Streaming args display (generic view only)
  const argsStr = bubble.args
    ? JSON.stringify(bubble.args, null, 2)
    : bubble.argsRaw || "";
  const argsStreaming = !bubble.args && bubble.argsRaw.length > 0;
  const resultStr =
    !isInline && bubble.result && !fileResult
      ? JSON.stringify(bubble.result, null, 2)
      : null;

  if (isInline) {
    return (
      <div className={styles.toolRow}>
        <div className={styles.toolBubbleInline}>
          {/* Header */}
          <div className={styles.toolHeaderInline}>
            <span className={styles.toolIcon} aria-hidden="true">
              {toolIcon}
            </span>
            <span className={styles.toolName}>{toolLabel}</span>
            {bubble.pending && (
              <span className={styles.toolPending}>运行中…</span>
            )}
          </div>

          {/* Specialised body */}
          {!bubble.pending && isTerminal && (
            <TerminalView
              toolName={bubble.name}
              command={args?.command ? String(args.command) : undefined}
              stdout={result?.stdout ? String(result.stdout) : undefined}
              stderr={result?.stderr ? String(result.stderr) : undefined}
              exitCode={
                result?.exit_code !== undefined
                  ? (result.exit_code as number | null)
                  : undefined
              }
            />
          )}

          {!bubble.pending && isDiff && bubble.name === "str_replace" && (
            <DiffView
              mode="str_replace"
              path={String(args!.path)}
              oldStr={String(args!.old_str)}
              newStr={String(args!.new_str)}
            />
          )}

          {!bubble.pending && isDiff && bubble.name === "write" && (
            <DiffView
              mode="write"
              path={String(args!.path)}
              newStr={String(args!.content)}
            />
          )}

          {/* Streaming args while pending */}
          {bubble.pending && argsStr && (
            <div className={styles.toolSection}>
              <pre className={styles.toolCode}>
                {argsStr}
                {argsStreaming && (
                  <span className={styles.cursor} aria-hidden="true" />
                )}
              </pre>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={styles.toolRow}>
      <div className={styles.toolBubble}>
        <button
          className={styles.toolHeader}
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
        >
          <span className={styles.toolIcon} aria-hidden="true">
            {toolIcon}
          </span>
          <span className={styles.toolName}>{bubble.name}</span>
          {bubble.pending && (
            <span className={styles.toolPending}>运行中…</span>
          )}
          <span className={styles.toolChevron} aria-hidden="true">
            {expanded ? "▲" : "▼"}
          </span>
        </button>

        {expanded && (
          <div className={styles.toolBody}>
            {argsStr && (
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>参数</p>
                <pre className={styles.toolCode}>
                  {argsStr}
                  {argsStreaming && (
                    <span className={styles.cursor} aria-hidden="true" />
                  )}
                </pre>
              </div>
            )}
            {resultStr !== null && (
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>结果</p>
                <pre className={styles.toolCode}>{resultStr}</pre>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── File card (Claude-style) ─────────────────────────────────────────────────

interface FileInfo {
  display: "file";
  filename: string;
  path: string;
  size: number;
}

type PreviewState =
  | { status: "idle" }
  | { status: "loading" }
  | {
      status: "ready";
      content: string;
      lang: string;
      lineCount: number;
      truncated: boolean;
    }
  | { status: "error"; message: string }
  | { status: "binary" };

function FileCard({ file, pending }: { file: FileInfo; pending: boolean }) {
  const [preview, setPreview] = useState<PreviewState>({ status: "idle" });
  const [expanded, setExpanded] = useState(false);

  const token = localStorage.getItem("familiar_token") ?? "";

  const loadPreview = useCallback(async () => {
    if (preview.status !== "idle") return;
    setPreview({ status: "loading" });
    try {
      const params = new URLSearchParams({ path: file.path, token });
      const res = await fetch(`/api/files/preview?${params}`);
      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: "无法预览" }));
        if (res.status === 400) {
          setPreview({ status: "binary" });
        } else {
          setPreview({ status: "error", message: err.error ?? "加载失败" });
        }
        return;
      }
      const data = await res.json();
      setPreview({
        status: "ready",
        content: data.content,
        lang: data.lang,
        lineCount: data.line_count,
        truncated: data.truncated,
      });
    } catch {
      setPreview({ status: "error", message: "网络错误" });
    }
  }, [file.path, token, preview.status]);

  function toggleExpand() {
    if (!expanded && preview.status === "idle") {
      loadPreview();
    }
    setExpanded((v) => !v);
  }

  const handleDownload = useCallback(() => {
    const params = new URLSearchParams({ path: file.path, token });
    const a = document.createElement("a");
    a.href = `/api/files?${params}`;
    a.download = file.filename;
    a.click();
  }, [file.path, file.filename, token]);

  const ext = file.filename.includes(".")
    ? file.filename.split(".").pop()!.toLowerCase()
    : "";

  return (
    <div className={styles.toolRow}>
      <div
        className={`${styles.fileCard} ${pending ? styles.fileCardPending : ""}`}
      >
        {/* ── Card header ── */}
        <div className={styles.fileCardHeader}>
          <div className={styles.fileCardLeft}>
            <span className={styles.fileCardIcon} aria-hidden="true">
              {fileEmoji(ext)}
            </span>
            <div className={styles.fileCardMeta}>
              <span className={styles.fileCardName}>{file.filename}</span>
              {!pending && (
                <span className={styles.fileCardSize}>
                  {formatBytes(file.size)}
                </span>
              )}
              {pending && (
                <span className={styles.fileCardPendingLabel}>准备中…</span>
              )}
            </div>
          </div>

          <div className={styles.fileCardActions}>
            {!pending && (
              <>
                <button
                  className={styles.fileCardBtn}
                  onClick={toggleExpand}
                  aria-label={expanded ? "收起预览" : "展开预览"}
                  title={expanded ? "收起" : "预览"}
                >
                  <EyeIcon />
                  <span>{expanded ? "收起" : "预览"}</span>
                </button>
                <button
                  className={`${styles.fileCardBtn} ${styles.fileCardBtnPrimary}`}
                  onClick={handleDownload}
                  aria-label={`下载 ${file.filename}`}
                  title="下载"
                >
                  <DownloadIcon />
                  <span>下载</span>
                </button>
              </>
            )}
          </div>
        </div>

        {/* ── Preview area ── */}
        {expanded && (
          <div className={styles.filePreview}>
            {preview.status === "loading" && (
              <div className={styles.filePreviewLoading}>加载中…</div>
            )}
            {preview.status === "binary" && (
              <div className={styles.filePreviewBinary}>
                <span aria-hidden="true">📦</span>
                <span>二进制文件，请下载后查看</span>
              </div>
            )}
            {preview.status === "error" && (
              <div className={styles.filePreviewError}>
                ⚠️ {preview.message}
              </div>
            )}
            {preview.status === "ready" && (
              <>
                <FilePreviewContent
                  content={preview.content}
                  lang={preview.lang}
                  lineCount={preview.lineCount}
                />
                {preview.truncated && (
                  <div className={styles.filePreviewTruncated}>
                    文件过大，仅显示前 100 KB
                  </div>
                )}
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── File preview content (with highlight.js) ─────────────────────────────────

function FilePreviewContent({
  content,
  lang,
  lineCount,
}: {
  content: string;
  lang: string;
  lineCount: number;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  // Import hljs dynamically to keep the main bundle lean — it's already loaded
  // by MarkdownRenderer so this will hit the module cache.
  useEffect(() => {
    import("highlight.js").then((hljs) => {
      const el = containerRef.current?.querySelector("code");
      if (!el) return;
      if (lang && hljs.default.getLanguage(lang)) {
        el.innerHTML = hljs.default.highlight(content, {
          language: lang,
        }).value;
      } else {
        el.innerHTML = hljs.default.highlightAuto(content).value;
      }
    });
  }, [content, lang]);

  return (
    <div ref={containerRef} className={styles.filePreviewCode}>
      <div className={styles.filePreviewCodeHeader}>
        {lang && <span className={styles.filePreviewLang}>{lang}</span>}
        <span className={styles.filePreviewLines}>{lineCount} 行</span>
      </div>
      <pre className={styles.filePreviewPre}>
        <code className={`hljs ${lang ? `language-${lang}` : ""}`}>
          {content}
        </code>
      </pre>
    </div>
  );
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function fileEmoji(ext: string): string {
  const map: Record<string, string> = {
    rs: "🦀",
    py: "🐍",
    js: "📜",
    ts: "📘",
    tsx: "📘",
    jsx: "📜",
    go: "🐹",
    md: "📝",
    json: "📋",
    toml: "⚙️",
    yaml: "⚙️",
    yml: "⚙️",
    sh: "💻",
    bash: "💻",
    sql: "🗄️",
    html: "🌐",
    css: "🎨",
    png: "🖼️",
    jpg: "🖼️",
    jpeg: "🖼️",
    gif: "🖼️",
    svg: "🖼️",
    pdf: "📕",
    zip: "📦",
    tar: "📦",
    gz: "📦",
    log: "📋",
  };
  return map[ext] ?? "📄";
}

// ─── Icons ────────────────────────────────────────────────────────────────────

function EyeIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  );
}
