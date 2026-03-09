//! Thin client for the OpenRouter embeddings endpoint.
//!
//! Uses `openai/text-embedding-3-small` (1536 dimensions).
//! Called with `tokio::task::spawn_blocking` is NOT needed here — reqwest is async.

use reqwest::Client;
use serde::{Deserialize, Serialize};

const EMBEDDING_URL: &str = "https://openrouter.ai/api/v1/embeddings";
const EMBEDDING_MODEL: &str = "openai/text-embedding-3-small";

#[derive(Clone)]
pub struct EmbeddingClient {
    client: Client,
    token: String,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

impl EmbeddingClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            token: token.into(),
        }
    }

    /// Embed a single string. Returns a 1536-dimensional vector.
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let resp: EmbedResponse = self
            .client
            .post(EMBEDDING_URL)
            .bearer_auth(&self.token)
            .json(&EmbedRequest {
                model: EMBEDDING_MODEL,
                input: text,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| anyhow::anyhow!("empty embedding response"))
    }
}

/// Cosine similarity between two equal-length vectors.
/// Returns a value in [-1, 1]; higher = more similar.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
