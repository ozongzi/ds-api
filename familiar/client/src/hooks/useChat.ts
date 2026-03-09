import { useCallback, useRef, useState } from "react";
import type { ChatBubble, WsServerEvent } from "../api/types";

type ChatStatus = "idle" | "connecting" | "streaming" | "error";

function uid() {
  return Math.random().toString(36).slice(2);
}

export function useChat(conversationId: string | null, token: string | null) {
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [status, setStatus] = useState<ChatStatus>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  // Key of the assistant bubble currently being streamed
  const streamKeyRef = useRef<string | null>(null);

  // Load historical messages as bubbles (called once when a conversation opens)
  const setHistory = useCallback(
    (msgs: Array<{ role: string; content: string | null }>) => {
      const history: ChatBubble[] = msgs
        .filter((m) => m.role === "user" || m.role === "assistant")
        .filter((m) => m.content && m.content.trim().length > 0)
        .map((m) => ({
          key: uid(),
          role: m.role as "user" | "assistant",
          content: m.content!,
          streaming: false,
        }));
      setBubbles(history);
    },
    []
  );

  const clearBubbles = useCallback(() => {
    setBubbles([]);
    setStatus("idle");
    setErrorMsg(null);
  }, []);

  const send = useCallback(
    (text: string) => {
      if (!conversationId || !token) return;
      if (status === "connecting" || status === "streaming") return;

      setErrorMsg(null);

      // ── Optimistically add user bubble ──────────────────────────────────
      const userKey = uid();
      setBubbles((prev) => [
        ...prev,
        { key: userKey, role: "user", content: text, streaming: false },
      ]);

      // ── Open WebSocket ──────────────────────────────────────────────────
      setStatus("connecting");

      // Use wss:// when page is served over https
      const wsProtocol = location.protocol === "https:" ? "wss" : "ws";
      const ws = new WebSocket(
        `${wsProtocol}://${location.host}/ws/${conversationId}`
      );
      wsRef.current = ws;

      ws.addEventListener("open", () => {
        // Step 1: auth handshake
        ws.send(JSON.stringify({ token }));
        // Step 2: user message
        ws.send(JSON.stringify({ content: text }));
        setStatus("streaming");

        // Prepare an empty assistant bubble to stream into
        const assistantKey = uid();
        streamKeyRef.current = assistantKey;
        setBubbles((prev) => [
          ...prev,
          {
            key: assistantKey,
            role: "assistant",
            content: "",
            streaming: true,
          },
        ]);
      });

      ws.addEventListener("message", (ev) => {
        let event: WsServerEvent;
        try {
          event = JSON.parse(ev.data as string) as WsServerEvent;
        } catch {
          return;
        }

        if (event.type === "token") {
          const key = streamKeyRef.current;
          if (!key) return;
          setBubbles((prev) =>
            prev.map((b) =>
              b.key === key ? { ...b, content: b.content + event.content } : b
            )
          );
        } else if (event.type === "done") {
          // Mark the streaming bubble as complete
          const key = streamKeyRef.current;
          if (key) {
            setBubbles((prev) =>
              prev.map((b) => (b.key === key ? { ...b, streaming: false } : b))
            );
            streamKeyRef.current = null;
          }
          setStatus("idle");
          ws.close();
          wsRef.current = null;
        } else if (event.type === "error") {
          const key = streamKeyRef.current;
          if (key) {
            // Remove the empty / partial assistant bubble on error
            setBubbles((prev) => prev.filter((b) => b.key !== key));
            streamKeyRef.current = null;
          }
          setStatus("error");
          setErrorMsg(event.message);
          ws.close();
          wsRef.current = null;
        }
        // tool_call and tool_result events are silently ignored in the UI
        // (they are internal agent mechanics)
      });

      ws.addEventListener("error", () => {
        const key = streamKeyRef.current;
        if (key) {
          setBubbles((prev) => prev.filter((b) => b.key !== key));
          streamKeyRef.current = null;
        }
        setStatus("error");
        setErrorMsg("连接出错，请重试");
        wsRef.current = null;
      });

      ws.addEventListener("close", (ev) => {
        // Abnormal close (code !== 1000 / 1001) that wasn't already handled
        if (ev.code !== 1000 && ev.code !== 1001) {
          const key = streamKeyRef.current;
          if (key) {
            setBubbles((prev) => prev.filter((b) => b.key !== key));
            streamKeyRef.current = null;
          }
          if (status !== "error") {
            setStatus("error");
            setErrorMsg("连接已断开");
          }
        }
        wsRef.current = null;
      });
    },
    [conversationId, token, status]
  );

  const abort = useCallback(() => {
    wsRef.current?.close();
    wsRef.current = null;
    const key = streamKeyRef.current;
    if (key) {
      // Keep whatever was streamed so far, just mark as no longer streaming
      setBubbles((prev) =>
        prev.map((b) => (b.key === key ? { ...b, streaming: false } : b))
      );
      streamKeyRef.current = null;
    }
    setStatus("idle");
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
