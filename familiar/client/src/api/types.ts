// ─── Auth ─────────────────────────────────────────────────────────────────

export interface LoginRequest {
  name: string;
  password: string;
}

export interface LoginResponse {
  token: string;
}

export interface RegisterRequest {
  name: string;
  password: string;
}

export interface RegisterResponse {
  id: string;
  name: string;
  is_admin: boolean;
  created_at: string;
}

export interface MeResponse {
  id: string;
  name: string;
  is_admin: boolean;
  created_at: string;
}

// ─── Conversations ────────────────────────────────────────────────────────

export interface Conversation {
  id: string;
  name: string;
  created_at: string;
}

export interface CreateConversationRequest {
  name?: string;
}

export interface RenameConversationRequest {
  name: string;
}

// ─── Messages ─────────────────────────────────────────────────────────────

export interface Message {
  id: number;
  role: "user" | "assistant" | "system" | "tool";
  name: string | null;
  content: string | null;
  tool_calls: string | null;
  tool_call_id: string | null;
  is_summary: boolean;
  created_at: number;
}

// ─── WebSocket events ─────────────────────────────────────────────────────

export type WsClientMsg = { token: string } | { content: string };

export type WsServerEvent =
  | { type: "token"; content: string }
  | { type: "tool_call_start"; id: string; name: string }
  | { type: "tool_call_args_delta"; id: string; delta: string }
  | { type: "tool_call"; id: string; name: string; args: unknown }
  | { type: "tool_result"; id: string; name: string; result: unknown }
  | { type: "done" }
  | { type: "error"; message: string };

// ─── UI-only chat bubble ──────────────────────────────────────────────────

export type BubbleRole = "user" | "assistant" | "tool";

export interface TextBubble {
  kind: "text";
  key: string;
  role: "user" | "assistant";
  content: string;
  streaming: boolean;
}

export interface ToolBubble {
  kind: "tool";
  key: string;
  role: "tool";
  name: string;
  /** Fully parsed args object, set when tool_call (final) event arrives */
  args: unknown;
  /** Raw args JSON string being streamed in character by character */
  argsRaw: string;
  result: unknown | null;
  /** Still waiting for the tool_result event */
  pending: boolean;
}

export type ChatBubble = TextBubble | ToolBubble;

// ─── API error shape ──────────────────────────────────────────────────────

export interface ApiError {
  error: string;
}
