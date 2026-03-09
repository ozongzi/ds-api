import { memo } from "react";
import { MarkdownRenderer } from "./MarkdownRenderer";
import type { ChatBubble } from "../api/types";
import styles from "./MessageBubble.module.css";

interface Props {
  bubble: ChatBubble;
}

export const MessageBubble = memo(function MessageBubble({ bubble }: Props) {
  const isUser = bubble.role === "user";

  return (
    <div className={`${styles.row} ${isUser ? styles.rowUser : styles.rowAssistant}`}>
      {!isUser && (
        <div className={styles.avatar} aria-hidden="true">
          🐱
        </div>
      )}

      <div className={`${styles.bubble} ${isUser ? styles.bubbleUser : styles.bubbleAssistant}`}>
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
        <div className={`${styles.avatar} ${styles.avatarUser}`} aria-hidden="true">
          你
        </div>
      )}
    </div>
  );
});
