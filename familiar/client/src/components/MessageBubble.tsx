import { memo, useState, useCallback } from "react";
import { MarkdownRenderer } from "./MarkdownRenderer";
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

  const argsStr = bubble.args ? JSON.stringify(bubble.args, null, 2) : "";

  // Check if result is a present_file response
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

  const resultStr =
    bubble.result && !fileResult
      ? JSON.stringify(bubble.result, null, 2)
      : null;

  const handleDownload = useCallback(() => {
    if (!fileResult) return;
    // Build a URL with the token from localStorage for auth
    const token = localStorage.getItem("familiar_token");
    const params = new URLSearchParams({ path: fileResult.path });
    if (token) params.set("token", token);
    const url = `/api/files?${params.toString()}`;
    const a = document.createElement("a");
    a.href = url;
    a.download = fileResult.filename;
    a.click();
  }, [fileResult]);

  return (
    <div className={styles.toolRow}>
      <div className={styles.toolBubble}>
        <button
          className={styles.toolHeader}
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
        >
          <span className={styles.toolIcon} aria-hidden="true">
            {bubble.pending ? "⚙️" : "✅"}
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
                <pre className={styles.toolCode}>{argsStr}</pre>
              </div>
            )}
            {fileResult && (
              <div className={styles.toolSection}>
                <p className={styles.toolSectionLabel}>文件</p>
                <div className={styles.fileDownload}>
                  <span className={styles.fileIcon} aria-hidden="true">
                    📄
                  </span>
                  <div className={styles.fileMeta}>
                    <span className={styles.fileName}>
                      {fileResult.filename}
                    </span>
                    <span className={styles.fileSize}>
                      {formatBytes(fileResult.size)}
                    </span>
                  </div>
                  <button
                    className={styles.downloadBtn}
                    onClick={handleDownload}
                    aria-label={`下载 ${fileResult.filename}`}
                  >
                    <DownloadIcon />
                    下载
                  </button>
                </div>
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

// ─── Helpers ──────────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
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
