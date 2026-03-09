/// Runtime configuration loaded from environment variables.
///
/// Set variables directly or place them in a `.env` file — `dotenvy` loads it
/// automatically before `from_env()` reads them.
pub struct Config {
    /// Discord Bot token (from the Developer Portal).
    pub discord_token: String,

    /// DeepSeek API key.
    pub deepseek_token: String,

    /// OpenRouter API key — used for embeddings (openai/text-embedding-3-small).
    pub openrouter_token: String,

    /// Optional system prompt prepended to every conversation.
    pub system_prompt: Option<String>,

    /// Path to the SQLite database file. Default: `familiar.db`
    pub db_path: String,
}

impl Config {
    /// Load configuration from the process environment.
    ///
    /// Attempts to read a `.env` file in the current directory first (via
    /// `dotenvy`), then reads the actual environment.  Missing required
    /// variables panic with a clear message so misconfiguration is caught at
    /// startup rather than at first use.
    pub fn from_env() -> Self {
        let _ = dotenvy::dotenv();

        Self {
            discord_token: required("DISCORD_TOKEN"),
            deepseek_token: required("DEEPSEEK_API_KEY"),
            openrouter_token: required("OPENROUTER_API_KEY"),
            system_prompt: std::env::var("SYSTEM_PROMPT").ok(),
            db_path: std::env::var("DB_PATH").unwrap_or_else(|_| "familiar.db".to_string()),
        }
    }
}

fn required(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("required environment variable `{key}` is not set"))
}
