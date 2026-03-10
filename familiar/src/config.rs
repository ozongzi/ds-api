use config::{Config as Cfg, Environment, File};
use serde::Deserialize;

/// Top-level configuration for familiar.
///
/// Loading order (later sources override earlier ones):
///   1. `config.toml`  — all settings including secrets, git-ignored
///   2. Environment variables prefixed with `FAMILIAR__`
///      e.g. `FAMILIAR__SECRETS__DEEPSEEK_API_KEY=sk-...`
///           `FAMILIAR__SERVER__PORT=8080`
#[derive(Debug, Deserialize)]
pub struct Config {
    pub secrets: Secrets,
    pub model: ModelConfig,
    pub embedding: EmbeddingConfig,
    pub server: ServerConfig,
    #[serde(default)]
    pub mcp: Vec<McpServerConfig>,
}

/// Sensitive credentials.
#[derive(Debug, Deserialize)]
pub struct Secrets {
    pub deepseek_api_key: String,
    pub openrouter_api_key: String,
    pub database_url: String,
}

/// LLM model configuration.
#[derive(Debug, Deserialize)]
pub struct ModelConfig {
    pub api_base: String,
    pub name: String,
}

/// Embedding model configuration.
#[derive(Debug, Deserialize)]
pub struct EmbeddingConfig {
    pub api_base: String,
    pub model: String,
}

/// HTTP server configuration.
#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    /// Path to a file whose contents become the system prompt.
    pub system_prompt_file: Option<String>,
}

/// A single MCP server to launch at startup.
#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Config {
    pub fn load() -> Self {
        let cfg = Cfg::builder()
            .add_source(File::with_name("config").required(true))
            .add_source(
                Environment::with_prefix("FAMILIAR")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()
            .expect("failed to build configuration");

        cfg.try_deserialize().expect("invalid configuration")
    }

    /// Read the system prompt from disk if `server.system_prompt_file` is set.
    pub fn system_prompt(&self) -> Option<String> {
        let path = self.server.system_prompt_file.as_deref()?;
        match std::fs::read_to_string(path) {
            Ok(s) => Some(s.trim().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => panic!("failed to read system_prompt_file '{path}': {e}"),
        }
    }
}
