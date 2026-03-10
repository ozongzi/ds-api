import { useCallback, useRef, useState } from "react";
import type {
  ChatBubble,
  TextBubble,
  ToolBubble,
  WsServerEvent,
} from "../api/types";

type ChatStatus = "idle" | "connecting" | "streaming" | "error";

// During streaming the user can either inject a message mid-run or abort.
export type InterruptMode = "interrupt" | "abort";

function uid() {
  return Math.random().toString(36).slice(2);
}

export function useChat(conversationId: string | null, token: string | null) {
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [status, setStatus] = useState<ChatStatus>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  // Stable ref so abort/interrupt callbacks never go stale.
  const wsLiveRef = useRef<WebSocket | null>(null);
  // Track which conversationId we last attached to, to avoid double-attach.
  const attachedConvRef = useRef<string | null>(null);
  // True while a reattach WS is open but generation has not yet started
  // (status is still "idle"). Used by send() to detect and close it first.
  const reattachingRef = useRef(false);

  // True once setHistory has been called for the current conversation.
  // reattach() will not open a WS until this is true.
  const historyReadyRef = useRef(false);

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
      reasoning: "",
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
          reasoning: "",
          streaming: false,
        }));
      setBubbles(history);
      historyReadyRef.current = true;
    },
    [],
  );

  const clearBubbles = useCallback(() => {
    setBubbles([]);
    activeTextKeyRef.current = null;
    updateStatus("idle");
    setErrorMsg(null);
    attachedConvRef.current = null;
    historyReadyRef.current = false;
  }, []);

  // ── Interrupt / abort (usable while streaming) ─────────────────────────

  const interrupt = useCallback((text: string) => {
    const ws = wsLiveRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    if (statusRef.current !== "streaming") return;

    // Show the injected message immediately as a user bubble.
    const userBubble: TextBubble = {
      kind: "text",
      key: uid(),
      role: "user",
      content: text,
      reasoning: "",
      streaming: false,
    };
    setBubbles((prev) => [...prev, userBubble]);

    ws.send(JSON.stringify({ type: "interrupt", content: text }));
  }, []);

  const abort = useCallback(() => {
    const ws = wsLiveRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: "abort" }));
  }, []);

  // ── Core WebSocket event processor (shared by send and reattach) ───────

  /**
   * Process a single parsed WsServerEvent, mutating bubble state.
   * Returns true if the event signals end-of-stream (done/aborted/error).
   */
  const processEvent = useCallback((event: WsServerEvent): boolean => {
    if (event.type === "user_interrupt") {
      return false;
    } else if (event.type === "aborted") {
      sealActiveText();
      updateStatus("idle");
      return true;
    } else if (event.type === "reasoning_token") {
      const key = ensureActiveText();
      setBubbles((prev) =>
        prev.map((b) =>
          b.key === key && b.kind === "text"
            ? { ...b, reasoning: b.reasoning + event.content }
            : b,
        ),
      );
    } else if (event.type === "token") {
      const key = ensureActiveText();
      setBubbles((prev) =>
        prev.map((b) =>
          b.key === key && b.kind === "text"
            ? { ...b, content: b.content + event.content }
            : b,
        ),
      );
    } else if (event.type === "tool_call_start") {
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
      setBubbles((prev) => {
        // Avoid duplicates during replay.
        if (prev.some((b) => b.key === `tool-${event.id}`)) return prev;
        return [...prev, toolBubble];
      });
    } else if (event.type === "tool_call_args_delta") {
      setBubbles((prev) =>
        prev.map((b) =>
          b.key === `tool-${event.id}` && b.kind === "tool"
            ? { ...b, argsRaw: b.argsRaw + event.delta }
            : b,
        ),
      );
    } else if (event.type === "tool_call") {
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
    } else if (event.type === "done") {
      sealActiveText();
      updateStatus("idle");
      return true;
    } else if (event.type === "error") {
      const key = activeTextKeyRef.current;
      if (key) {
        setBubbles((prev) => prev.filter((b) => b.key !== key));
        activeTextKeyRef.current = null;
      }
      updateStatus("error");
      setErrorMsg(event.message);
      return true;
    }
    return false;
  }, []);

  // ── Reattach to an ongoing generation — called explicitly by ChatPage
  //    after history has been loaded, so there is no race between
  //    setHistory overwriting bubbles and replay events being processed.

  const reattach = useCallback(
    (conversationId: string, token: string) => {
      // Guard: don't open a second WS if one is already live.
      if (wsLiveRef.current) return;
      if (attachedConvRef.current === conversationId) return;

      attachedConvRef.current = conversationId;

      const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
      const ws = new WebSocket(
        `${wsProtocol}://${location.host}/ws/${conversationId}`,
      );
      wsRef.current = ws;
      wsLiveRef.current = ws;

      ws.addEventListener("open", () => {
        reattachingRef.current = true;
        ws.send(JSON.stringify({ token }));
        ws.send(JSON.stringify({ type: "reattach" }));
      });

      ws.addEventListener("message", (ev) => {
        let event: WsServerEvent;
        try {
          event = JSON.parse(ev.data as string) as WsServerEvent;
        } catch {
          return;
        }

        // During reattach we set status to streaming as soon as we see any
        // non-terminal event, so the UI shows the in-progress state.
        if (
          statusRef.current === "idle" &&
          event.type !== "done" &&
          event.type !== "aborted" &&
          event.type !== "error"
        ) {
          reattachingRef.current = false;
          updateStatus("streaming");
        }

        const finished = processEvent(event);
        if (finished) {
          reattachingRef.current = false;
          ws.close(1000);
          wsRef.current = null;
          wsLiveRef.current = null;
        }
      });

      ws.addEventListener("error", () => {
        reattachingRef.current = false;
        wsRef.current = null;
        wsLiveRef.current = null;
      });

      ws.addEventListener("close", () => {
        reattachingRef.current = false;
        wsRef.current = null;
        wsLiveRef.current = null;
      });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  // ── Open a new WebSocket turn ──────────────────────────────────────────

  const send = useCallback(
    (text: string) => {
      if (!conversationId || !token) return;
      if (
        statusRef.current === "connecting" ||
        statusRef.current === "streaming"
      )
        return;

      // 关闭 reattach 阶段可能留下的静默连接，避免两个 WS 同时追加 token。
      // reattachingRef 标记的是"已建连但 generation 尚未开始"的静默连接；
      // 如果 generation 已经在跑（streaming），send 在函数开头就已被拦截。
      if (reattachingRef.current) {
        const existing = wsLiveRef.current;
        if (existing && existing.readyState === WebSocket.OPEN) {
          existing.close(1000);
        }
        reattachingRef.current = false;
        wsRef.current = null;
        wsLiveRef.current = null;
        attachedConvRef.current = null;
      }

      setErrorMsg(null);
      activeTextKeyRef.current = null;

      // Optimistically add user bubble
      const userBubble: TextBubble = {
        kind: "text",
        key: uid(),
        role: "user",
        content: text,
        reasoning: "",
        streaming: false,
      };
      setBubbles((prev) => [...prev, userBubble]);

      updateStatus("connecting");

      const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
      const ws = new WebSocket(
        `${wsProtocol}://${location.host}/ws/${conversationId}`,
      );
      wsRef.current = ws;
      wsLiveRef.current = ws;
      attachedConvRef.current = conversationId;

      ws.addEventListener("open", () => {
        ws.send(JSON.stringify({ token }));
        ws.send(JSON.stringify({ content: text }));
        updateStatus("streaming");
      });

      ws.addEventListener("message", (ev) => {
        let event: WsServerEvent;
        try {
          event = JSON.parse(ev.data as string) as WsServerEvent;
        } catch {
          return;
        }

        const finished = processEvent(event);
        if (finished) {
          ws.close(1000);
          wsRef.current = null;
          wsLiveRef.current = null;
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
        wsLiveRef.current = null;
      });

      ws.addEventListener("close", (ev) => {
        wsRef.current = null;
        wsLiveRef.current = null;
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

  return {
    bubbles,
    status,
    errorMsg,
    send,
    interrupt,
    abort,
    reattach,
    setHistory,
    clearBubbles,
  };
}
