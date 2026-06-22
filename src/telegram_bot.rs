//! Telegram side, built on teloxide.
//!
//! Commands: /krappe, /naamat, /top [all], /combine <irc nick>.

use crate::config::TelegramConfig;
use crate::core;
use crate::db::{self, PLATFORM_TELEGRAM};
use sqlx::SqlitePool;
use teloxide::prelude::*;
use teloxide::types::User;
use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Krappebot-komennot:")]
enum Command {
    #[command(description = "lisää yksi krappe (ja anna +v IRCissä)")]
    Krappe,
    #[command(description = "olet sammunut – saat hauskan admin-tittelin")]
    Naamat,
    #[command(description = "krappe-tilasto (lisää 'all' kaikkien aikojen listalle)")]
    Top(String),
    #[command(description = "kippis jollain kielellä")]
    Kalja,
    #[command(description = "kannustusta krapulaiselle nousuhumalan tielle")]
    Nousuun,
    #[command(description = "yhdistä Telegram-tilisi IRC-nimimerkkiin: /combine <nick>")]
    Combine(String),
}

/// Entry point: run the Telegram dispatcher until the process stops.
pub async fn run(cfg: TelegramConfig, pool: SqlitePool) -> anyhow::Result<()> {
    let bot = Bot::new(cfg.token);
    tracing::info!("starting Telegram bot");

    let handler = Update::filter_message()
        .filter_command::<Command>()
        .endpoint(answer);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![pool])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

/// A stable key (the numeric user id) and a human display name for a Telegram user.
fn user_key_and_display(user: &User) -> (String, String) {
    let key = user.id.0.to_string();
    let display = match &user.username {
        Some(name) => format!("@{name}"),
        None => user.first_name.clone(),
    };
    (key, display)
}

async fn answer(bot: Bot, msg: Message, cmd: Command, pool: SqlitePool) -> ResponseResult<()> {
    let Some(user) = msg.from.clone() else {
        return Ok(()); // channel post or service message without a sender
    };
    let (user_key, display) = user_key_and_display(&user);

    match cmd {
        Command::Krappe => {
            match db::record_krappe(&pool, PLATFORM_TELEGRAM, &user_key, &display).await {
                Ok(count) => {
                    let text = format!("🍺 {display} otti krappen! Yhteensä: {count}");
                    bot.send_message(msg.chat.id, text).await?;
                }
                Err(e) => {
                    tracing::error!(error = %e, "record_krappe failed");
                    bot.send_message(msg.chat.id, "Krappen tallennus epäonnistui 😵")
                        .await?;
                }
            }
        }

        Command::Naamat => {
            let title = core::random_naamat_title();
            // Try the real trick: promote (with no meaningful rights) then set a custom title.
            // This only works if the bot is a group admin with can_promote_members.
            let promoted = bot
                .promote_chat_member(msg.chat.id, user.id)
                .can_invite_users(true)
                .await
                .is_ok();
            let titled = if promoted {
                bot.set_chat_administrator_custom_title(msg.chat.id, user.id, title.to_string())
                    .await
                    .is_ok()
            } else {
                false
            };

            let text = if titled {
                format!("👑 {display} on virallisesti SAMMUNUT. Uusi arvonimi: «{title}» 🍺")
            } else {
                // Fallback when the bot lacks admin rights or it's a private chat.
                format!("💀🍺 {display} on niin naamat, ettei pysy pystyssä! (botti ei ole admin, joten titteli jäi antamatta)")
            };
            bot.send_message(msg.chat.id, text).await?;
        }

        Command::Top(arg) => {
            let scope = core::parse_scope(&arg);
            match db::leaderboard(&pool, scope, 20).await {
                Ok(entries) => {
                    let text = core::format_leaderboard(&core::scope_header(scope), &entries);
                    bot.send_message(msg.chat.id, text).await?;
                }
                Err(e) => {
                    tracing::error!(error = %e, "leaderboard failed");
                    bot.send_message(msg.chat.id, "Tilaston haku epäonnistui 😵")
                        .await?;
                }
            }
        }

        Command::Kalja => {
            bot.send_message(msg.chat.id, core::random_cheers()).await?;
        }

        Command::Nousuun => {
            bot.send_message(msg.chat.id, core::random_nousuun()).await?;
        }

        Command::Combine(arg) => {
            let nick = arg.trim();
            if nick.is_empty() || nick.contains(char::is_whitespace) {
                bot.send_message(msg.chat.id, "Käyttö: /combine <irc-nick>")
                    .await?;
            } else {
                match db::link_combine(&pool, &user_key, nick).await {
                    Ok(()) => {
                        let text = format!(
                            "🔗 {display} yhdistetty IRC-nimimerkkiin «{}». Krappet lasketaan nyt yhteen.",
                            nick.to_lowercase()
                        );
                        bot.send_message(msg.chat.id, text).await?;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "link_combine failed");
                        bot.send_message(msg.chat.id, "Yhdistäminen epäonnistui 😵")
                            .await?;
                    }
                }
            }
        }
    }

    Ok(())
}
