import { useCallback, useRef, useState } from "react";
import type {
  ChatBubble,
  TextBubble,
  ToolBubble,
  WsServerEvent,
} from "../api/types";

type ChatStatus = "idle" | "connecting" | "streaming" | "error";

function uid() {
  return Math.random().toString(36).slice(2);
}

export function useChat(conversationId: string | null, token: string | null) {
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [status, setStatus] = useState<ChatStatus>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const wsRef = useRef<WebSocket | null>(null);

  // Key of the assistant TextBubble that is currently accumulating tokens.
  // null means no active text segment yet (next token will create one).
  const activeTextKeyRef = useRef<string | null>(null);

  // statusRef so close/error handlers always read the latest value
  // without stale-closure issues.
  const statusRef = useRef<ChatStatus>("idle");

  function updateStatus(s: ChatStatus) {
    statusRef.current = s;
    setStatus(s);
  }

  // ── Helpers ────────────────────────────────────────────────────────────────

  /** Seal the current active text bubble (stop streaming). */
  function sealActiveText() {
    const key = activeTextKeyRef.current;
    if (!key) return;
    setBubbles((prev) =>
      prev.map((b) =>
        b.key === key && b.kind === "text" ? { ...b, streaming: false } : b,
      ),
    );
    activeTextKeyRef.current = null;
  }

  /**
   * Ensure there is an active streaming text bubble for the assistant.
   * If one already exists, returns its key; otherwise creates a new one
   * and appends it to the list.
   */
  function ensureActiveText(): string {
    if (activeTextKeyRef.current) return activeTextKeyRef.current;
    const key = uid();
    activeTextKeyRef.current = key;
    const bubble: TextBubble = {
      kind: "text",
      key,
      role: "assistant",
      content: "",
      streaming: true,
    };
    setBubbles((prev) => [...prev, bubble]);
    return key;
  }

  // ── Public API ─────────────────────────────────────────────────────────────

  const setHistory = useCallback(
    (msgs: Array<{ role: string; content: string | null }>) => {
      const history: TextBubble[] = msgs
        .filter((m) => m.role === "user" || m.role === "assistant")
        .filter((m) => m.content && m.content.trim().length > 0)
        .map((m) => ({
          kind: "text" as const,
          key: uid(),
          role: m.role as "user" | "assistant",
          content: m.content!,
          streaming: false,
        }));
      setBubbles(history);
    },
    [],
  );

  const clearBubbles = useCallback(() => {
    setBubbles([]);
    activeTextKeyRef.current = null;
    updateStatus("idle");
    setErrorMsg(null);
  }, []);

  const send = useCallback(
    (text: string) => {
      if (!conversationId || !token) return;
      if (
        statusRef.current === "connecting" ||
        statusRef.current === "streaming"
      )
        return;

      setErrorMsg(null);
      activeTextKeyRef.current = null;

      // Optimistically add user bubble
      const userBubble: TextBubble = {
        kind: "text",
        key: uid(),
        role: "user",
        content: text,
        streaming: false,
      };
      setBubbles((prev) => [...prev, userBubble]);

      updateStatus("connecting");

      const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
      const ws = new WebSocket(
        `${wsProtocol}://${location.host}/ws/${conversationId}`,
      );
      wsRef.current = ws;

      ws.addEventListener("open", () => {
        ws.send(JSON.stringify({ token }));
        ws.send(JSON.stringify({ content: text }));
        updateStatus("streaming");
        // Don't pre-create an assistant bubble here — we create it lazily
        // on the first token, so the order is always correct.
      });

      ws.addEventListener("message", (ev) => {
        let event: WsServerEvent;
        try {
          event = JSON.parse(ev.data as string) as WsServerEvent;
        } catch {
          return;
        }

        if (event.type === "token") {
          // Append to the current active text segment, creating one if needed.
          const key = ensureActiveText();
          setBubbles((prev) =>
            prev.map((b) =>
              b.key === key && b.kind === "text"
                ? { ...b, content: b.content + event.content }
                : b,
            ),
          );
        } else if (event.type === "tool_call_start") {
          // Streaming: tool name is known, args not yet arrived.
          sealActiveText();
          const toolBubble: ToolBubble = {
            kind: "tool",
            key: `tool-${event.id}`,
            role: "tool",
            name: event.name,
            args: null,
            argsRaw: "",
            result: null,
            pending: true,
          };
          setBubbles((prev) => [...prev, toolBubble]);
        } else if (event.type === "tool_call_args_delta") {
          // Streaming: append raw args fragment to the existing bubble.
          setBubbles((prev) =>
            prev.map((b) =>
              b.key === `tool-${event.id}` && b.kind === "tool"
                ? { ...b, argsRaw: b.argsRaw + event.delta }
                : b,
            ),
          );
        } else if (event.type === "tool_call") {
          // Final event: full parsed args arrive (or first event in non-streaming).
          // If tool_call_start already created the bubble, update args in place.
          // If not (non-streaming path), create the bubble now.
          sealActiveText();
          setBubbles((prev) => {
            const exists = prev.some(
              (b) => b.key === `tool-${event.id}` && b.kind === "tool",
            );
            if (exists) {
              return prev.map((b) =>
                b.key === `tool-${event.id}` && b.kind === "tool"
                  ? { ...b, args: event.args }
                  : b,
              );
            }
            const toolBubble: ToolBubble = {
              kind: "tool",
              key: `tool-${event.id}`,
              role: "tool",
              name: event.name,
              args: event.args,
              argsRaw: "",
              result: null,
              pending: true,
            };
            return [...prev, toolBubble];
          });
        } else if (event.type === "tool_result") {
          setBubbles((prev) =>
            prev.map((b) =>
              b.key === `tool-${event.id}` && b.kind === "tool"
                ? { ...b, result: event.result, pending: false }
                : b,
            ),
          );
          // After a tool result the agent may emit more tokens — ensureActiveText
          // will create a new text bubble for them automatically.
        } else if (event.type === "done") {
          sealActiveText();
          updateStatus("idle");
          ws.close(1000);
          wsRef.current = null;
        } else if (event.type === "error") {
          // Remove the current (possibly empty) active text bubble on error.
          const key = activeTextKeyRef.current;
          if (key) {
            setBubbles((prev) => prev.filter((b) => b.key !== key));
            activeTextKeyRef.current = null;
          }
          updateStatus("error");
          setErrorMsg(event.message);
          ws.close(1000);
          wsRef.current = null;
        }
      });

      ws.addEventListener("error", () => {
        const key = activeTextKeyRef.current;
        if (key) {
          setBubbles((prev) => prev.filter((b) => b.key !== key));
          activeTextKeyRef.current = null;
        }
        updateStatus("error");
        setErrorMsg("连接出错，请重试");
        wsRef.current = null;
      });

      ws.addEventListener("close", (ev) => {
        wsRef.current = null;
        // Only treat as an error if the close was abnormal AND we are still
        // in the streaming state (i.e. done/error has not already handled it).
        if (
          ev.code !== 1000 &&
          ev.code !== 1001 &&
          statusRef.current === "streaming"
        ) {
          const key = activeTextKeyRef.current;
          if (key) {
            setBubbles((prev) => prev.filter((b) => b.key !== key));
            activeTextKeyRef.current = null;
          }
          updateStatus("error");
          setErrorMsg("连接已断开，请重试");
        }
      });
    },
    [conversationId, token],
  );

  const abort = useCallback(() => {
    wsRef.current?.close(1000);
    wsRef.current = null;
    sealActiveText();
    updateStatus("idle");
  }, []);

  return {
    bubbles,
    status,
    errorMsg,
    send,
    abort,
    setHistory,
    clearBubbles,
  };
}
