pub struct Config {
    /// DeepSeek API key.
    pub deepseek_token: String,

    /// OpenRouter API key — used for embeddings (openai/text-embedding-3-small).
    pub openrouter_token: String,

    /// Optional system prompt prepended to every conversation.
    pub system_prompt: Option<String>,

    /// PostgreSQL connection URL. e.g. postgres://user:pass@localhost/familiar
    pub database_url: String,

    /// Port to listen on. Default: 3000
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        let _ = dotenvy::dotenv();

        Self {
            deepseek_token: required("DEEPSEEK_API_KEY"),
            openrouter_token: required("OPENROUTER_API_KEY"),
            system_prompt: std::env::var("SYSTEM_PROMPT").ok(),
            database_url: required("DATABASE_URL"),
            port: std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
        }
    }
}

fn required(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("required environment variable `{key}` is not set"))
}
