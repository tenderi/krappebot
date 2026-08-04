//! Telegram side, built on teloxide.
//!
//! Commands: /krappe, /naamat, /top [all], /stat [nick] [all], /combine <irc nick>
//! (repeatable — each call adds another nick to the account's identity),
//! /uncombine <irc nick> (undoes one).

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
    #[command(description = "tilastot: /stat [nick] [all] (ilman nickiä omat tilastosi)")]
    Stat(String),
    #[command(description = "kippis jollain kielellä")]
    Kalja,
    #[command(description = "kannustusta krapulaiselle nousuhumalan tielle")]
    Nousuun,
    #[command(description = "yhdistä IRC-nimimerkki tiliisi: /combine <nick> (voit ajaa monta kertaa)")]
    Combine(String),
    #[command(description = "peru yhdistäminen: /uncombine <nick>")]
    Uncombine(String),
}

/// Entry point: run the Telegram dispatcher until the process stops.
pub async fn run(cfg: TelegramConfig, pool: SqlitePool) -> anyhow::Result<()> {
    let bot = Bot::new(cfg.token);
    tracing::info!("starting Telegram bot");

    // Publish the command list so Telegram shows it in the client's "/" menu.
    if let Err(e) = bot.set_my_commands(Command::bot_commands()).await {
        tracing::warn!(error = %e, "failed to register bot commands with Telegram");
    }

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
            match db::record_krappe_daily(&pool, PLATFORM_TELEGRAM, &user_key, &display).await {
                Ok(db::KrappeOutcome::Recorded(count)) => {
                    let text = format!("{display} otti krappen! Yhteensä: {count}");
                    bot.send_message(msg.chat.id, text).await?;
                }
                Ok(db::KrappeOutcome::AlreadyToday(count)) => {
                    let text = format!("{display}: {} (Yhteensä: {count})", core::random_shame());
                    bot.send_message(msg.chat.id, text).await?;
                }
                Err(e) => {
                    tracing::error!(error = %e, "record_krappe failed");
                    bot.send_message(msg.chat.id, "Krappen tallennus epäonnistui.")
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
                format!("{display} on naamat, kunnollista! Uusi arvonimi: «{title}»")
            } else {
                // Fallback when the bot lacks admin rights or it's a private chat.
                format!("👑 {display} on naamat, kunnollista!")
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
                    bot.send_message(msg.chat.id, "Tilaston haku epäonnistui.")
                        .await?;
                }
            }
        }

        Command::Stat(arg) => {
            let (nick_arg, all) = core::parse_stat_args(arg.split_whitespace());
            let (canon, name) = match nick_arg {
                Some(nick) => {
                    let c = core::canonical_irc_nick(nick);
                    (c.clone(), c)
                }
                None => match db::canonical_key(&pool, PLATFORM_TELEGRAM, &user_key).await {
                    Ok(c) => {
                        let name = if c.starts_with("tg:") { display.clone() } else { c.clone() };
                        (c, name)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "canonical_key failed");
                        bot.send_message(msg.chat.id, "Tilaston haku epäonnistui.").await?;
                        return Ok(());
                    }
                },
            };
            let reply = core::stat_reply(&pool, &canon, &name, all).await;
            bot.send_message(msg.chat.id, reply).await?;
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
                // Store the same canonical form the IRC events use, so counts merge.
                let canon = core::canonical_irc_nick(nick);
                // First /combine sets up the primary link; later ones add the extra
                // nick as an alias onto whatever the account already resolves to, so
                // repeated /combine accumulates nicks instead of replacing the link.
                let result = match db::telegram_link(&pool, &user_key).await {
                    Ok(None) => db::link_combine(&pool, &user_key, &canon).await,
                    Ok(Some(existing)) if existing == canon => Ok(()),
                    Ok(Some(_)) => match db::canonical_key(&pool, PLATFORM_TELEGRAM, &user_key).await {
                        Ok(current) => db::add_nick_alias(&pool, &canon, &current).await,
                        Err(e) => Err(e),
                    },
                    Err(e) => Err(e),
                };
                match result {
                    Ok(()) => {
                        let text = format!(
                            "{display}: nick «{canon}» yhdistetty. Krappet lasketaan nyt yhteen."
                        );
                        bot.send_message(msg.chat.id, text).await?;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "combine failed");
                        bot.send_message(msg.chat.id, "Yhdistäminen epäonnistui.")
                            .await?;
                    }
                }
            }
        }

        Command::Uncombine(arg) => {
            let nick = arg.trim();
            if nick.is_empty() || nick.contains(char::is_whitespace) {
                bot.send_message(msg.chat.id, "Käyttö: /uncombine <nick>").await?;
            } else {
                let canon = core::canonical_irc_nick(nick);
                match db::remove_nick_alias(&pool, &canon).await {
                    Ok(true) => {
                        let text = format!("Nick «{canon}» erotettu, lasketaan taas erikseen.");
                        bot.send_message(msg.chat.id, text).await?;
                    }
                    Ok(false) => {
                        // Not a manually merged nick -- maybe it's the primary link itself.
                        match db::telegram_link(&pool, &user_key).await {
                            Ok(Some(primary)) if primary == canon => {
                                match db::unlink_telegram(&pool, &user_key).await {
                                    Ok(_) => {
                                        let text = format!(
                                            "{display}: tili irrotettu nimimerkistä «{canon}»."
                                        );
                                        bot.send_message(msg.chat.id, text).await?;
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "unlink_telegram failed");
                                        bot.send_message(msg.chat.id, "Erottaminen epäonnistui.")
                                            .await?;
                                    }
                                }
                            }
                            _ => {
                                let text = format!("Nick «{canon}» ei ollut yhdistetty.");
                                bot.send_message(msg.chat.id, text).await?;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "remove_nick_alias failed");
                        bot.send_message(msg.chat.id, "Erottaminen epäonnistui.").await?;
                    }
                }
            }
        }
    }

    Ok(())
}
