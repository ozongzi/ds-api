use std::sync::Arc;

use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use ds_api::AgentEvent;
use futures::{SinkExt, StreamExt};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::errors::AppError;
use crate::web::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(conversation_id): Path<Uuid>,
) -> Result<Response, AppError> {
    // Verify conversation exists (ownership is checked via the token passed
    // over the WebSocket on first message — see below).
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1)")
            .bind(conversation_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| {
                tracing::error!("ws conversation lookup: {e}");
                AppError::internal("数据库错误")
            })?;

    if !exists {
        return Err(AppError::not_found("对话不存在"));
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, conversation_id)))
}

async fn handle_socket(socket: WebSocket, state: AppState, conversation_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();

    // ── Auth handshake ────────────────────────────────────────────────────────
    // First message must be: { "token": "<bearer>" }
    let user_id = match receiver.next().await {
        Some(Ok(Message::Text(txt))) => {
            let v: serde_json::Value = match serde_json::from_str(&txt) {
                Ok(v) => v,
                Err(_) => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"invalid auth message"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            };
            let token = match v.get("token").and_then(|t| t.as_str()) {
                Some(t) => t.to_string(),
                None => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"missing token"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            };

            // Verify token owns this conversation.
            match sqlx::query(
                r#"
                SELECT c.user_id
                FROM sessions s
                JOIN conversations c ON c.user_id = s.user_id
                WHERE s.token = $1 AND c.id = $2
                "#,
            )
            .bind(token)
            .bind(conversation_id)
            .fetch_optional(&state.pool)
            .await
            {
                Ok(Some(row)) => row.try_get::<Uuid, _>("user_id").unwrap_or(Uuid::nil()),
                Ok(None) => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"unauthorized"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
                Err(e) => {
                    tracing::error!("ws auth query: {e}");
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"db error"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            }
        }
        _ => return,
    };

    // ── Wait for a user message ───────────────────────────────────────────────
    // Second message: { "content": "<text>" }
    let user_text = match receiver.next().await {
        Some(Ok(Message::Text(txt))) => {
            let v: serde_json::Value = match serde_json::from_str(&txt) {
                Ok(v) => v,
                Err(_) => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"invalid message"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            };
            match v
                .get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
            {
                Some(t) if !t.trim().is_empty() => t,
                _ => {
                    let _ = sender
                        .send(Message::Text(
                            json!({"type":"error","message":"empty content"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    return;
                }
            }
        }
        _ => return,
    };

    // ── Persist user message ──────────────────────────────────────────────────
    {
        use ds_api::raw::request::message::{Message as AgentMessage, Role};
        let msg = AgentMessage::new(Role::User, &user_text);
        state.persist_message(conversation_id, &msg);
    }

    // ── Acquire or create agent ───────────────────────────────────────────────
    let agent_opt = state
        .with_chat_async(conversation_id, |entry| {
            if entry.agent.is_some() {
                entry.agent.take()
            } else {
                let _ = entry.interrupt_tx.send(user_text.clone());
                None
            }
        })
        .await;

    let Some(mut agent) = agent_opt else {
        let _ = sender
            .send(Message::Text(
                json!({"type":"error","message":"agent busy"})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };

    agent.push_user_message_with_name(&user_text, None);

    // ── Stream events ─────────────────────────────────────────────────────────
    let state = Arc::new(state);
    let state_clone = Arc::clone(&state);

    let mut stream = agent.chat_from_history();
    let mut reply_buf = String::new();

    while let Some(event) = stream.next().await {
        let msg_text = match event {
            Ok(AgentEvent::Token(token)) => {
                reply_buf.push_str(&token);
                json!({"type": "token", "content": token}).to_string()
            }
            Ok(AgentEvent::ToolCall(info)) => json!({
                "type": "tool_call",
                "id": info.id,
                "name": info.name,
                "args": info.args,
            })
            .to_string(),
            Ok(AgentEvent::ToolResult(res)) => json!({
                "type": "tool_result",
                "id": res.id,
                "name": res.name,
                "result": res.result,
            })
            .to_string(),
            Err(e) => {
                tracing::error!(conversation = %conversation_id, "agent error: {e}");
                let _ = sender
                    .send(Message::Text(
                        json!({"type":"error","message": e.to_string()})
                            .to_string()
                            .into(),
                    ))
                    .await;

                if let Some(recovered) = stream.into_agent() {
                    state_clone
                        .with_chat_async(conversation_id, |entry| {
                            entry.agent = Some(recovered);
                        })
                        .await;
                }
                return;
            }
        };

        if sender.send(Message::Text(msg_text.into())).await.is_err() {
            // Client disconnected — recover agent and bail.
            if let Some(recovered) = stream.into_agent() {
                state_clone
                    .with_chat_async(conversation_id, |entry| {
                        entry.agent = Some(recovered);
                    })
                    .await;
            }
            return;
        }
    }

    // ── Recover agent ─────────────────────────────────────────────────────────
    if let Some(recovered) = stream.into_agent() {
        state_clone
            .with_chat_async(conversation_id, |entry| {
                entry.agent = Some(recovered);
            })
            .await;
    }

    // ── Persist assistant reply ───────────────────────────────────────────────
    if !reply_buf.is_empty() {
        use ds_api::raw::request::message::{Message as AgentMessage, Role};
        let msg = AgentMessage::new(Role::Assistant, &reply_buf);
        state_clone.persist_message(conversation_id, &msg);
    }

    // ── Send done ─────────────────────────────────────────────────────────────
    let _ = sender
        .send(Message::Text(json!({"type":"done"}).to_string().into()))
        .await;

    // Ignore user_id — used only for auth verification above.
    let _ = user_id;
}
