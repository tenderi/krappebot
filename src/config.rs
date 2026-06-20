//! Configuration loaded from environment variables (optionally via a `.env` file).

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub telegram: Option<TelegramConfig>,
    pub irc: Option<IrcConfig>,
}

#[derive(Clone, Debug)]
pub struct TelegramConfig {
    pub token: String,
}

#[derive(Clone, Debug)]
pub struct IrcConfig {
    pub server: String,
    pub port: u16,
    pub use_tls: bool,
    pub nickname: String,
    pub channels: Vec<String>,
    pub nickserv_password: Option<String>,
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

impl Config {
    /// Load config. Telegram and IRC are each enabled only if their required
    /// variables are present, so you can run just one platform during testing.
    pub fn from_env() -> Result<Self> {
        let database_url =
            env_opt("DATABASE_URL").unwrap_or_else(|| "sqlite://krappe.db".to_string());

        let telegram = env_opt("TELOXIDE_TOKEN").map(|token| TelegramConfig { token });

        let irc = match (env_opt("IRC_SERVER"), env_opt("IRC_NICK")) {
            (Some(server), Some(nickname)) => {
                let port = env_opt("IRC_PORT")
                    .map(|p| p.parse::<u16>())
                    .transpose()
                    .context("IRC_PORT must be a number")?
                    .unwrap_or(6697);
                let use_tls = env_opt("IRC_USE_TLS")
                    .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
                    .unwrap_or(true);
                let channels = env_opt("IRC_CHANNELS")
                    .map(|c| c.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default();
                Some(IrcConfig {
                    server,
                    port,
                    use_tls,
                    nickname,
                    channels,
                    nickserv_password: env_opt("IRC_NICKSERV_PASSWORD"),
                })
            }
            _ => None,
        };

        Ok(Config {
            database_url,
            telegram,
            irc,
        })
    }
}
