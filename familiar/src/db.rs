//! Persistent conversation history backed by SQLite.
//!
//! # Schema
//!
//! ```sql
//! messages        — one row per Message, append-only
//! messages_fts    — FTS5 virtual table shadowing messages.content
//! ```
//!
//! # Design
//!
//! - All blocking SQLite calls run inside `tokio_rusqlite::Connection`, which
//!   dispatches them to a dedicated thread so the async runtime is never blocked.
//! - Embeddings are stored as a little-endian f32 blob (1536 × 4 = 6144 bytes).
//! - On restore, we find the latest summary row and load only
//!   [that summary + everything after it] to keep the in-memory history bounded.

use std::path::Path;

use rusqlite::{OptionalExtension, params};
use tokio_rusqlite::Connection;

use ds_api::raw::request::message::{Message, Role};

// ── Row type ──────────────────────────────────────────────────────────────────

/// A single persisted message row.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MessageRow {
    pub id: i64,
    pub channel_id: String,
    pub role: String,
    pub name: Option<String>,
    pub content: Option<String>,
    /// Serialised JSON, nullable.
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub is_summary: bool,
    pub created_at: i64,
    /// Raw f32-le blob, length = EMBEDDING_DIMS * 4. May be empty.
    pub embedding: Vec<u8>,
}

// ── Db handle ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at `path` and run migrations.
    pub async fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_owned();
        let conn = Connection::open(path).await?;

        conn.call(|c| {
            c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

            c.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS messages (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    channel_id    TEXT    NOT NULL,
                    role          TEXT    NOT NULL,
                    name          TEXT,
                    content       TEXT,
                    tool_calls    TEXT,
                    tool_call_id  TEXT,
                    is_summary    INTEGER NOT NULL DEFAULT 0,
                    created_at    INTEGER NOT NULL,
                    embedding     BLOB    NOT NULL DEFAULT (X'')
                );

                CREATE INDEX IF NOT EXISTS idx_channel_id
                    ON messages (channel_id, id);

                CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
                USING fts5(
                    content,
                    content='messages',
                    content_rowid='id'
                );

                -- Keep FTS in sync via triggers.
                CREATE TRIGGER IF NOT EXISTS messages_ai
                AFTER INSERT ON messages BEGIN
                    INSERT INTO messages_fts(rowid, content)
                    VALUES (new.id, COALESCE(new.content, ''));
                END;

                CREATE TRIGGER IF NOT EXISTS messages_ad
                AFTER DELETE ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, content)
                    VALUES ('delete', old.id, COALESCE(old.content, ''));
                END;

                CREATE TRIGGER IF NOT EXISTS messages_au
                AFTER UPDATE ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, content)
                    VALUES ('delete', old.id, COALESCE(old.content, ''));
                    INSERT INTO messages_fts(rowid, content)
                    VALUES (new.id, COALESCE(new.content, ''));
                END;
                "#,
            )?;

            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("db migration failed: {e}"))?;

        Ok(Self { conn })
    }

    // ── Write ─────────────────────────────────────────────────────────────────

    /// Persist a single `Message` for `channel_id`.
    /// Returns the new row id.
    pub async fn append(
        &self,
        channel_id: &str,
        msg: &Message,
        embedding: Vec<u8>,
    ) -> anyhow::Result<i64> {
        let channel_id = channel_id.to_owned();
        let role = role_to_str(&msg.role).to_owned();
        let name = msg.name.clone();
        let content = msg.content.clone();
        let tool_calls = msg
            .tool_calls
            .as_ref()
            .and_then(|tc| serde_json::to_string(tc).ok());
        let tool_call_id = msg.tool_call_id.clone();
        let is_summary = msg.is_auto_summary() as i64;
        let now = unix_now();

        let id = self
            .conn
            .call(move |c| {
                c.execute(
                    r#"INSERT INTO messages
                        (channel_id, role, name, content, tool_calls, tool_call_id,
                         is_summary, created_at, embedding)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
                    params![
                        channel_id,
                        role,
                        name,
                        content,
                        tool_calls,
                        tool_call_id,
                        is_summary,
                        now,
                        embedding,
                    ],
                )?;
                Ok(c.last_insert_rowid())
            })
            .await?;

        Ok(id)
    }

    /// Update the embedding for an existing row by id.
    pub async fn set_embedding(&self, row_id: i64, embedding: Vec<u8>) -> anyhow::Result<()> {
        self.conn
            .call(move |c| {
                c.execute(
                    "UPDATE messages SET embedding = ?1 WHERE id = ?2",
                    params![embedding, row_id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    // ── Restore ───────────────────────────────────────────────────────────────

    /// Load the messages needed to reconstruct the in-memory agent history for
    /// `channel_id`:
    ///
    /// 1. Find the most recent summary row.
    /// 2. Return [that summary row] + [all rows after it].
    ///
    /// If there is no summary, return all rows.
    pub async fn restore(&self, channel_id: &str) -> anyhow::Result<Vec<Message>> {
        let channel_id = channel_id.to_owned();

        let rows: Vec<MessageRow> = self
            .conn
            .call(move |c| {
                // Find latest summary id for this channel.
                let summary_id: Option<i64> = c
                    .query_row(
                        r#"SELECT id FROM messages
                           WHERE channel_id = ?1 AND is_summary = 1
                           ORDER BY id DESC LIMIT 1"#,
                        params![channel_id],
                        |row| row.get(0),
                    )
                    .optional()?;

                let since_id = summary_id.unwrap_or(0);

                let mut stmt = c.prepare(
                    r#"SELECT id, channel_id, role, name, content,
                              tool_calls, tool_call_id, is_summary, created_at, embedding
                       FROM messages
                       WHERE channel_id = ?1 AND id >= ?2
                       ORDER BY id ASC"#,
                )?;

                let rows = stmt
                    .query_map(params![channel_id, since_id], row_to_message_row)?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(rows)
            })
            .await?;

        Ok(rows.into_iter().map(row_to_message).collect())
    }

    // ── FTS5 search ───────────────────────────────────────────────────────────

    /// Full-text search over `content` using SQLite FTS5.
    ///
    /// Returns up to `limit` matching `MessageRow`s, most recent first.
    pub async fn fts_search(
        &self,
        channel_id: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MessageRow>> {
        let channel_id = channel_id.to_owned();
        let query = query.to_owned();

        let rows = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare(
                    r#"SELECT m.id, m.channel_id, m.role, m.name, m.content,
                              m.tool_calls, m.tool_call_id, m.is_summary,
                              m.created_at, m.embedding
                       FROM messages_fts f
                       JOIN messages m ON m.id = f.rowid
                       WHERE f.content MATCH ?1
                         AND m.channel_id = ?2
                       ORDER BY m.id DESC
                       LIMIT ?3"#,
                )?;

                let rows = stmt
                    .query_map(params![query, channel_id, limit as i64], row_to_message_row)?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(rows)
            })
            .await?;

        Ok(rows)
    }

    /// Load all rows for `channel_id` that have a non-empty embedding blob,
    /// for use in semantic (vector) search.
    pub async fn all_embeddings(
        &self,
        channel_id: &str,
    ) -> anyhow::Result<Vec<(i64, Vec<u8>, Option<String>)>> {
        let channel_id = channel_id.to_owned();

        let rows = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare(
                    r#"SELECT id, embedding, content
                       FROM messages
                       WHERE channel_id = ?1
                         AND length(embedding) > 0
                       ORDER BY id ASC"#,
                )?;

                let rows = stmt
                    .query_map(params![channel_id], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(rows)
            })
            .await?;

        Ok(rows)
    }

    /// Fetch full `MessageRow`s by their ids.
    pub async fn rows_by_ids(&self, ids: &[i64]) -> anyhow::Result<Vec<MessageRow>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let ids = ids.to_vec();

        let rows = self
            .conn
            .call(move |c| {
                // Build a parameterised IN clause dynamically.
                let placeholders: String = ids
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", i + 1))
                    .collect::<Vec<_>>()
                    .join(", ");

                let sql = format!(
                    r#"SELECT id, channel_id, role, name, content,
                              tool_calls, tool_call_id, is_summary, created_at, embedding
                       FROM messages
                       WHERE id IN ({placeholders})
                       ORDER BY id ASC"#
                );

                let mut stmt = c.prepare(&sql)?;

                let params_iter: Vec<&dyn rusqlite::ToSql> =
                    ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();

                let rows = stmt
                    .query_map(params_iter.as_slice(), row_to_message_row)?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(rows)
            })
            .await?;

        Ok(rows)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn role_to_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn str_to_role(s: &str) -> Role {
    match s {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

fn row_to_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        id: row.get(0)?,
        channel_id: row.get(1)?,
        role: row.get(2)?,
        name: row.get(3)?,
        content: row.get(4)?,
        tool_calls: row.get(5)?,
        tool_call_id: row.get(6)?,
        is_summary: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
        embedding: row.get(9)?,
    })
}

fn row_to_message(row: MessageRow) -> Message {
    use ds_api::raw::request::message::ToolCall;

    let tool_calls: Option<Vec<ToolCall>> = row
        .tool_calls
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    Message {
        role: str_to_role(&row.role),
        content: row.content,
        name: row.name,
        tool_call_id: row.tool_call_id,
        tool_calls,
        reasoning_content: None,
        prefix: None,
    }
}

/// Encode a `Vec<f32>` as a little-endian byte blob.
pub fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for &x in v {
        buf.extend_from_slice(&x.to_le_bytes());
    }
    buf
}

/// Decode a little-endian byte blob back to `Vec<f32>`.
pub fn decode_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}
