//! krappebot — counts "krappe" (hangovers) across IRC and Telegram.
//!
//! Runs both bots as concurrent tasks sharing one SQLite store. Each platform is
//! enabled only if its config is present, so you can develop against one at a time.

mod config;
mod core;
mod db;
mod irc_bot;
mod telegram_bot;

use anyhow::Result;
use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,krappebot=info".into()),
        )
        .init();

    let cfg = Config::from_env()?;
    let pool = db::init(&cfg.database_url).await?;
    tracing::info!(db = %cfg.database_url, "database ready");

    if cfg.telegram.is_none() && cfg.irc.is_none() {
        anyhow::bail!(
            "No platform configured. Set TELOXIDE_TOKEN and/or IRC_SERVER+IRC_NICK in the environment."
        );
    }

    // Build the (optional) per-platform futures and run whichever are configured.
    let telegram = cfg.telegram.clone();
    let irc = cfg.irc.clone();
    let tg_pool = pool.clone();
    let irc_pool = pool.clone();

    let tg_task = tokio::spawn(async move {
        if let Some(tg) = telegram {
            if let Err(e) = telegram_bot::run(tg, tg_pool).await {
                tracing::error!(error = %e, "telegram bot stopped");
            }
        }
    });

    let irc_task = tokio::spawn(async move {
        if let Some(irc) = irc {
            if let Err(e) = irc_bot::run(irc, irc_pool).await {
                tracing::error!(error = %e, "irc bot stopped");
            }
        }
    });

    // If either task ends (error or disconnect), let the process exit so a
    // supervisor (systemd, docker restart) can restart it cleanly.
    let _ = tokio::try_join!(tg_task, irc_task);
    Ok(())
}
