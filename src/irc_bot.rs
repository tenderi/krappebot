//! IRC side, built on the `irc` crate.
//!
//! Commands (must be sent in a channel): !krappe, !naamat, !top [all].
//! !krappe grants the user +v; !naamat grants +o.

use crate::config::IrcConfig;
use crate::core;
use crate::db::{self, PLATFORM_IRC};
use futures::StreamExt;
use irc::client::prelude::{Client, Command, Config, Response};
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
    let mut joined = false;
    while let Some(message) = stream.next().await {
        let message = message?;
        match &message.command {
            // Join channels only once the server has fully registered us. Relying on
            // Config::channels auto-join races registration on some networks (e.g.
            // IRCnet), so we join explicitly. We trigger on the welcome (001) and also
            // on end-of-MOTD (376) / no-MOTD (422) as a fallback, guarded so we only
            // join once.
            Command::Response(code, args) => {
                tracing::debug!(?code, ?args, "server numeric");
                if !joined
                    && matches!(
                        code,
                        Response::RPL_WELCOME | Response::RPL_ENDOFMOTD | Response::ERR_NOMOTD
                    )
                {
                    joined = true;
                    for channel in &cfg.channels {
                        tracing::info!(%channel, "joining channel");
                        if let Err(e) = client.send_join(channel) {
                            tracing::warn!(error = %e, %channel, "failed to send JOIN");
                        }
                    }
                }
            }
            Command::PRIVMSG(target, text) => {
                // Only act on channel messages (target starts with '#'); ignore PMs.
                if !target.starts_with('#') {
                    continue;
                }
                let Some(nick) = message.source_nickname() else {
                    continue;
                };
                handle_command(&client, &pool, target, nick, text).await;
            }
            _ => {}
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
            let key = core::canonical_irc_nick(nick);
            match db::record_krappe_daily(pool, PLATFORM_IRC, &key, nick).await {
                Ok(db::KrappeOutcome::Recorded(count)) => {
                    set_mode(client, channel, ChannelMode::Voice, nick);
                    let _ = client.send_privmsg(
                        channel,
                        format!("{nick} otti krappen! Yhteensä: {count}"),
                    );
                }
                Ok(db::KrappeOutcome::AlreadyToday(count)) => {
                    let _ = client.send_privmsg(
                        channel,
                        format!("{nick}: {} (Yhteensä: {count})", core::random_shame()),
                    );
                }
                Err(e) => {
                    tracing::error!(error = %e, "record_krappe failed");
                    let _ = client.send_privmsg(channel, "Krappen tallennus epäonnistui.");
                }
            }
        }

        "!naamat" => {
            set_mode(client, channel, ChannelMode::Oper, nick);
            let _ = client.send_privmsg(channel, format!("{nick} on naamat, kunnollista!"));
        }

        "!kalja" => {
            let _ = client.send_privmsg(channel, core::random_cheers());
        }

        "!nousuun" => {
            let _ = client.send_privmsg(channel, core::random_nousuun());
        }

        "!top" => {
            let scope = core::parse_scope(parts.next().unwrap_or(""));
            match db::leaderboard(pool, scope, 20).await {
                Ok(entries) => {
                    // Single message: IRC can't put newlines in one PRIVMSG, so join inline.
                    let text =
                        core::format_leaderboard_inline(&core::scope_header(scope), &entries);
                    let _ = client.send_privmsg(channel, text);
                }
                Err(e) => {
                    tracing::error!(error = %e, "leaderboard failed");
                    let _ = client.send_privmsg(channel, "Tilaston haku epäonnistui.");
                }
            }
        }

        "!stat" => match parts.next() {
            None => {
                let _ = client.send_privmsg(channel, "Käyttö: !stat <nick> [all]");
            }
            Some(arg) => {
                let canon = core::canonical_irc_nick(arg);
                let all = parts.next().is_some_and(|a| a.eq_ignore_ascii_case("all"));
                let reply = core::stat_reply(pool, &canon, all).await;
                let _ = client.send_privmsg(channel, reply);
            }
        },

        _ => {}
    }
}

/// Set a channel mode for a single nick, e.g. +v or +o. Requires the bot to be opped.
fn set_mode(client: &Client, channel: &str, mode: ChannelMode, nick: &str) {
    if let Err(e) = client.send_mode(channel, &[Mode::Plus(mode, Some(nick.to_string()))]) {
        tracing::warn!(error = %e, channel, nick, "failed to set mode (is the bot opped?)");
    }
}
