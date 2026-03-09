use ds_api::tool;
use serde_json::json;

use crate::db::{Db, decode_embedding};
use crate::embedding::{EmbeddingClient, cosine_similarity};

pub struct HistoryTool {
    pub db: Db,
    pub embed: EmbeddingClient,
    pub channel_id: String,
}

#[tool]
impl Tool for HistoryTool {
    /// 用关键词全文搜索历史消息（FTS5，精确匹配）。
    /// 适合查找具体命令、文件名、错误信息等。
    /// query: 搜索关键词，支持 FTS5 语法，例如 "rust error" 或 "deploy AND nginx"
    /// limit: 返回最多几条结果，默认 10，最大 50
    async fn search_history_fts(&self, query: String, limit: Option<u32>) -> Value {
        let limit = limit.unwrap_or(10).min(50) as usize;

        match self.db.fts_search(&self.channel_id, &query, limit).await {
            Err(e) => json!({ "error": e.to_string() }),
            Ok(rows) => {
                let results: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "role": r.role,
                            "name": r.name,
                            "content": r.content,
                            "created_at": r.created_at,
                        })
                    })
                    .collect();
                let count = results.len();
                json!({ "results": results, "count": count })
            }
        }
    }

    /// 用自然语言语义搜索历史消息（向量相似度）。
    /// 适合模糊查找，例如"上次聊的那个网络问题"、"之前讨论的部署方案"。
    /// query: 自然语言描述，例如 "上次帮我装的软件"
    /// limit: 返回最多几条结果，默认 5，最大 20
    async fn search_history_semantic(&self, query: String, limit: Option<u32>) -> Value {
        let limit = limit.unwrap_or(5).min(20) as usize;

        // Embed the query.
        let query_vec: Vec<f32> = match self.embed.embed(&query).await {
            Ok(v) => v,
            Err(e) => return json!({ "error": format!("embedding failed: {e}") }),
        };

        // Load all stored embeddings for this channel.
        let all: Vec<(i64, Vec<u8>, Option<String>)> =
            match self.db.all_embeddings(&self.channel_id).await {
                Ok(v) => v,
                Err(e) => return json!({ "error": e.to_string() }),
            };

        if all.is_empty() {
            return json!({ "results": [], "count": 0 });
        }

        // Score each row.
        let mut scored: Vec<(i64, f32)> = all
            .into_iter()
            .map(|(id, blob, _content)| {
                let vec: Vec<f32> = decode_embedding(&blob);
                let score = cosine_similarity(&query_vec, &vec);
                (id, score)
            })
            .collect();

        // Sort descending by similarity.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        // Fetch full rows for the top hits.
        let top_ids: Vec<i64> = scored.iter().map(|(id, _)| *id).collect();
        let rows: Vec<crate::db::MessageRow> = match self.db.rows_by_ids(&top_ids).await {
            Ok(r) => r,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        // Build a score map for display.
        let score_map: std::collections::HashMap<i64, f32> = scored.into_iter().collect();

        let mut results: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                let score = score_map.get(&r.id).copied().unwrap_or(0.0);
                json!({
                    "id": r.id,
                    "role": r.role,
                    "name": r.name,
                    "content": r.content,
                    "created_at": r.created_at,
                    "similarity": (score * 1000.0).round() / 1000.0,
                })
            })
            .collect();

        // Re-sort by similarity descending (rows_by_ids returns by id ASC).
        results.sort_by(|a, b| {
            let sa = a["similarity"].as_f64().unwrap_or(0.0);
            let sb = b["similarity"].as_f64().unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        let count = results.len();
        json!({ "results": results, "count": count })
    }
}
