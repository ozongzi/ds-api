/// Runtime configuration loaded from environment variables.
///
/// Set variables directly or place them in a `.env` file — `dotenvy` loads it
/// automatically before `from_env()` reads them.
pub struct Config {
    /// Telegram Bot API token (from @BotFather).
    pub telegram_token: String,

    /// DeepSeek API key.
    pub deepseek_token: String,

    /// Port for the axum HTTP server.  Default: 8080.
    pub port: u16,

    /// Optional system prompt prepended to every conversation.
    pub system_prompt: Option<String>,

    /// Telegram secret token sent in the `X-Telegram-Bot-Api-Secret-Token`
    /// header by Telegram to verify that requests come from Telegram and not a
    /// third party.  Set this to any random string and pass the same value when
    /// registering the webhook with `setWebhook`.  If unset, the header check
    /// is skipped (fine for local dev, not recommended in production).
    pub webhook_secret: Option<String>,
}

impl Config {
    /// Load configuration from the process environment.
    ///
    /// Attempts to read a `.env` file in the current directory first (via
    /// `dotenvy`), then reads the actual environment.  Missing required
    /// variables panic with a clear message so misconfiguration is caught at
    /// startup rather than at first use.
    pub fn from_env() -> Self {
        // Load .env if present; ignore the error if the file doesn't exist.
        let _ = dotenvy::dotenv();

        let telegram_token = required("TELEGRAM_TOKEN");
        let deepseek_token = required("DEEPSEEK_API_KEY");

        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);

        let system_prompt = std::env::var("SYSTEM_PROMPT").ok();
        let webhook_secret = std::env::var("WEBHOOK_SECRET").ok();

        Self {
            telegram_token,
            deepseek_token,
            port,
            system_prompt,
            webhook_secret,
        }
    }
}

fn required(key: &str) -> String {
    std::env::var(key)
        .unwrap_or_else(|_| panic!("required environment variable `{key}` is not set"))
}
