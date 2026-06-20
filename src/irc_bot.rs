//! IRC side, built on the `irc` crate.
//!
//! Commands (must be sent in a channel): !krappe, !naamat, !top [all].
//! !krappe grants the user +v; !naamat grants +o.

use crate::config::IrcConfig;
use crate::core;
use crate::db::{self, PLATFORM_IRC};
use futures::StreamExt;
use irc::client::prelude::{Client, Command, Config};
use irc::proto::{ChannelMode, Mode};
use sqlx::SqlitePool;

/// Entry point: connect and process messages until the connection drops.
pub async fn run(cfg: IrcConfig, pool: SqlitePool) -> anyhow::Result<()> {
    let config = Config {
        nickname: Some(cfg.nickname.clone()),
        nick_password: cfg.nickserv_password.clone(),
        server: Some(cfg.server.clone()),
        port: Some(cfg.port),
        use_tls: Some(cfg.use_tls),
        channels: cfg.channels.clone(),
        ..Config::default()
    };

    let mut client = Client::from_config(config).await?;
    client.identify()?;
    tracing::info!(server = %cfg.server, "connected to IRC");

    let mut stream = client.stream()?;
    while let Some(message) = stream.next().await {
        let message = message?;
        if let Command::PRIVMSG(target, text) = &message.command {
            // Only act on channel messages (target starts with '#'); ignore PMs.
            if !target.starts_with('#') {
                continue;
            }
            let Some(nick) = message.source_nickname() else {
                continue;
            };
            handle_command(&client, &pool, target, nick, text).await;
        }
    }

    Ok(())
}

async fn handle_command(
    client: &Client,
    pool: &SqlitePool,
    channel: &str,
    nick: &str,
    text: &str,
) {
    let trimmed = text.trim();
    let mut parts = trimmed.split_whitespace();
    let Some(cmd) = parts.next() else { return };

    match cmd {
        "!krappe" => {
            let key = nick.to_lowercase();
            match db::record_krappe(pool, PLATFORM_IRC, &key, nick).await {
                Ok(count) => {
                    set_mode(client, channel, ChannelMode::Voice, nick);
                    let _ = client.send_privmsg(
                        channel,
                        format!("🍺 {nick} otti krappen! Yhteensä: {count}"),
                    );
                }
                Err(e) => {
                    tracing::error!(error = %e, "record_krappe failed");
                    let _ = client.send_privmsg(channel, "Krappen tallennus epäonnistui 😵");
                }
            }
        }

        "!naamat" => {
            set_mode(client, channel, ChannelMode::Oper, nick);
            let _ = client.send_privmsg(
                channel,
                format!("👑 {nick} on niin naamat että jakaa oppeja! +o annettu."),
            );
        }

        "!top" => {
            let scope = core::parse_scope(parts.next().unwrap_or(""));
            match db::leaderboard(pool, scope, 20).await {
                Ok(entries) => {
                    let text = core::format_leaderboard(&core::scope_header(scope), &entries);
                    // IRC lines can't contain newlines; send one PRIVMSG per line.
                    for line in text.lines() {
                        let _ = client.send_privmsg(channel, line);
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "leaderboard failed");
                    let _ = client.send_privmsg(channel, "Tilaston haku epäonnistui 😵");
                }
            }
        }

        _ => {}
    }
}

/// Set a channel mode for a single nick, e.g. +v or +o. Requires the bot to be opped.
fn set_mode(client: &Client, channel: &str, mode: ChannelMode, nick: &str) {
    if let Err(e) = client.send_mode(channel, &[Mode::Plus(mode, Some(nick.to_string()))]) {
        tracing::warn!(error = %e, channel, nick, "failed to set mode (is the bot opped?)");
    }
}
