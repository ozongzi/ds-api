import { type KeyboardEvent, useCallback, useRef, useEffect } from "react";
import styles from "./ChatInput.module.css";

interface Props {
  onSend: (text: string) => void;
  disabled?: boolean;
  placeholder?: string;
}

export function ChatInput({ onSend, disabled, placeholder }: Props) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-resize textarea to fit content
  const resize = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, []);

  useEffect(() => {
    resize();
  }, [resize]);

  const submit = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    const text = el.value.trim();
    if (!text || disabled) return;
    onSend(text);
    el.value = "";
    el.style.height = "auto";
  }, [onSend, disabled]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      // Enter sends; Shift+Enter inserts newline
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        submit();
      }
    },
    [submit]
  );

  return (
    <div className={styles.wrapper}>
      <div className={`${styles.box} ${disabled ? styles.boxDisabled : ""}`}>
        <textarea
          ref={textareaRef}
          className={styles.textarea}
          placeholder={placeholder ?? "发消息… (Enter 发送，Shift+Enter 换行)"}
          rows={1}
          onInput={resize}
          onKeyDown={handleKeyDown}
          disabled={disabled}
          aria-label="消息输入框"
        />
        <button
          className={styles.sendBtn}
          onClick={submit}
          disabled={disabled}
          aria-label="发送"
          title="发送 (Enter)"
        >
          <SendIcon />
        </button>
      </div>
      {disabled && (
        <p className={styles.hint}>正在生成回复…</p>
      )}
    </div>
  );
}

function SendIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="22" y1="2" x2="11" y2="13" />
      <polygon points="22 2 15 22 11 13 2 9 22 2" />
    </svg>
  );
}
