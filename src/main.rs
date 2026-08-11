//! krappebot — counts "krappe" (hangovers) across IRC and Telegram.
//!
//! Runs both bots as concurrent tasks sharing one SQLite store. Each platform is
//! enabled only if its config is present, so you can develop against one at a time.

use anyhow::Result;
use krappebot::config::Config;
use krappebot::{db, irc_bot, telegram_bot};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,krappebot=info".into()),
        )
        .init();

    // teloxide pulls rustls with aws-lc-rs, the irc crate pulls it with ring, and
    // cargo unifies them into one rustls that can no longer pick a provider on
    // its own — so it panics the first time TLS is used. Whichever task happened
    // to reach TLS first decided whether the process survived; install one here
    // so it is decided the same way every time.
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::debug!("rustls crypto provider was already installed");
    }

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

    // An unconfigured platform parks forever rather than returning, so it can't
    // trip the select below and take the configured one down with it.
    let tg_task = tokio::spawn(async move {
        match telegram {
            Some(tg) => telegram_bot::run(tg, tg_pool).await,
            None => std::future::pending().await,
        }
    });

    let irc_task = tokio::spawn(async move {
        match irc {
            Some(irc) => irc_bot::run(irc, irc_pool).await,
            None => std::future::pending().await,
        }
    });

    // Neither task is supposed to end. If one does, exit *non-zero* so systemd
    // actually restarts us — returning Ok here is what let a boot-time DNS
    // failure look like a clean shutdown and leave the bot dead until noticed.
    tokio::select! {
        r = tg_task => match r {
            Ok(Ok(())) => anyhow::bail!("telegram bot stopped unexpectedly"),
            Ok(Err(e)) => Err(e.context("telegram bot failed")),
            Err(e) => anyhow::bail!("telegram task failed: {e}"),
        },
        r = irc_task => match r {
            Ok(Ok(())) => anyhow::bail!("irc bot stopped unexpectedly"),
            Ok(Err(e)) => Err(e.context("irc bot failed")),
            Err(e) => anyhow::bail!("irc task failed: {e}"),
        },
    }
}
