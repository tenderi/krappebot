//! Platform-agnostic helpers shared by both bots and the importer: nick
//! canonicalization, command-argument parsing, leaderboard formatting, and the
//! various random phrase lists. Per project rule, none of these strings contain
//! emojis.

use crate::db::{LeaderEntry, NickStats, Scope};
use rand::seq::SliceRandom;

/// Collapse alt-nicks to a canonical key so the same person's krappe merge:
/// lowercase, drop away markers after '|' or '[', and strip trailing reconnect
/// markers (`_ - ` ^ \`). E.g. "Kukakumma_" and "kukakumma" both map to
/// "kukakumma". Used for IRC nicks at both write time and import.
pub fn canonical_irc_nick(nick: &str) -> String {
    let lower = nick.to_lowercase();
    let cut = match lower.find(['|', '[']) {
        Some(i) => &lower[..i],
        None => lower.as_str(),
    };
    let base = cut.trim_end_matches(['_', '-', '`', '^', '\\']);
    if base.is_empty() {
        lower
    } else {
        base.to_string()
    }
}

/// Custom admin titles for Telegram `/naamat` (Telegram caps these at 16 chars).
pub const NAAMAT_TITLES: &[&str] = &[
    "KRAPULA",
    "SAMMUNUT",
    "NAAMAT",
    "TÄYS KRAPULA",
    "VIINAHIRMU",
    "RÄKÄKÄNNISSÄ",
];

pub fn random_naamat_title() -> &'static str {
    NAAMAT_TITLES
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or("KRAPULA")
}

/// "Cheers!" in assorted languages for `!kalja` / `/kalja`.
pub const CHEERS: &[&str] = &[
    "Kippis!",
    "Skål!",
    "Cheers!",
    "Prost!",
    "Santé!",
    "Salud!",
    "Salute!",
    "Na zdrowie!",
    "Na zdorovie!",
    "Kanpai!",
    "Gānbēi!",
    "Sláinte!",
    "Proost!",
    "Saúde!",
    "Yamas!",
    "Şerefe!",
    "Egészségedre!",
    "Noroc!",
];

pub fn random_cheers() -> &'static str {
    CHEERS
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or("Kippis!")
}

/// Encouraging words for `!nousuun` / `/nousuun` — cheering on the weary as they
/// try to drink their way out of the hangover (nousuhumala).
pub const NOUSUUN_WORDS: &[&str] = &[
    "Yksi kalja vielä, niin nousuhumala iskee! Sisua peliin!",
    "Krapula on vain heikkojen tila. Nouse ja juo, sankari!",
    "Klaara lasi, niin maailma kirkastuu taas. Sinä pystyt tähän!",
    "Älä anna periksi - paras humala on nousuhumala. Kohti uusia seikkailuja!",
    "Pää kipeä? Lääke on tunnettu. Pohjat ja nousuun!",
    "Eilinen oli kova, mutta sinä olet kovempi. Yksi vielä ja lentoon!",
    "Vesilasi on petturi. Tartu kaljaan ja nouse tuhkasta kuin feeniks!",
    "Nousuhumala kutsuu. Vastaa rohkeasti - krapula kumartaa pian!",
];

pub fn random_nousuun() -> &'static str {
    NOUSUUN_WORDS
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or("Pohjat ja nousuun!")
}

/// Shaming lines for when someone tries to krappe twice on the same day.
pub const SHAME_WORDS: &[&str] = &[
    "Höpsis, otit jo krappen tänään. Yhden päivässä saa laskea!",
    "Kakkoskrappe ei kelpaa. Mene nukkumaan.",
    "Ahneeksi heittäydyit - yksi krappe per päivä riittää.",
    "Eikä eikä, tämän päivän krappe on jo kirjattu. Maltappa.",
    "Tuplakrappe? Eipä lasketa. Huomenna uudestaan.",
    "Jo riittää, krapulakuningas. Yksi merkintä per päivä.",
];

pub fn random_shame() -> &'static str {
    SHAME_WORDS
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or("Yksi krappe per päivä riittää.")
}

/// Interpret the argument to a top command: "all" -> all-time, anything else -> year.
pub fn parse_scope(arg: &str) -> Scope {
    if arg.trim().eq_ignore_ascii_case("all") {
        Scope::All
    } else {
        Scope::Year
    }
}

/// Render a leaderboard as a numbered list. `header` describes the scope.
pub fn format_leaderboard(header: &str, entries: &[LeaderEntry]) -> String {
    if entries.is_empty() {
        return format!("{header}: ei yhtään krappea vielä. Hienoa työtä!");
    }
    let mut out = format!("{header}:\n");
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&format!("{}. {} - {}\n", i + 1, e.display, e.count));
    }
    out.trim_end().to_string()
}

/// Like [`format_leaderboard`] but on a single line (entries joined by ", "),
/// for IRC where one PRIVMSG cannot contain newlines.
pub fn format_leaderboard_inline(header: &str, entries: &[LeaderEntry]) -> String {
    if entries.is_empty() {
        return format!("{header}: ei yhtään krappea vielä. Hienoa työtä!");
    }
    let body = entries
        .iter()
        .enumerate()
        .map(|(i, e)| format!("{}. {} - {}", i + 1, e.display, e.count))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{header}: {body}")
}

/// Default `!stat` / `/stat` output: current year first, all-time as context.
pub fn format_nick_stats(s: &NickStats) -> String {
    let year = chrono::Utc::now().format("%Y");
    let head = if s.year.count > 0 {
        format!(
            "{}: {year}: {} krappea (sija {}/{})",
            s.name, s.year.count, s.year.rank, s.year.people
        )
    } else {
        format!("{}: {year}: ei krappea vielä", s.name)
    };
    format!(
        "{head}. Kaikkiaan {} (sija {}/{}).",
        s.all.count, s.all.rank, s.all.people
    )
}

/// Build the `!stat` / `/stat` reply for a canonical nick. `all` selects the
/// per-year breakdown; otherwise the current-year-primary summary. Shared by
/// both bots so the wording stays identical.
pub async fn stat_reply(pool: &sqlx::SqlitePool, canon: &str, all: bool) -> String {
    if all {
        match crate::db::nick_yearly(pool, canon).await {
            Ok(yearly) => format_nick_yearly(canon, &yearly),
            Err(e) => {
                tracing::error!(error = %e, "nick_yearly failed");
                "Tilaston haku epäonnistui.".to_string()
            }
        }
    } else {
        match crate::db::nick_stats(pool, canon).await {
            Ok(Some(stats)) => format_nick_stats(&stats),
            Ok(None) => format!("{canon}: ei yhtään krappea."),
            Err(e) => {
                tracing::error!(error = %e, "nick_stats failed");
                "Tilaston haku epäonnistui.".to_string()
            }
        }
    }
}

/// `!stat <nick> all` output: per-year breakdown on one line.
pub fn format_nick_yearly(name: &str, yearly: &[(i32, i64)]) -> String {
    if yearly.is_empty() {
        return format!("{name}: ei yhtään krappea.");
    }
    let total: i64 = yearly.iter().map(|(_, c)| c).sum();
    let body = yearly
        .iter()
        .map(|(y, c)| format!("{y}: {c}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name} kautta aikojen: {body}. Yhteensä {total}.")
}

pub fn scope_header(scope: Scope) -> String {
    match scope {
        Scope::Year => format!("Krappe-tilasto {}", chrono::Utc::now().format("%Y")),
        Scope::All => "Krappe-tilasto (kaikkien aikojen)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_irc_nick;

    #[test]
    fn canonicalizes_alt_nicks() {
        assert_eq!(canonical_irc_nick("Kukakumma_"), "kukakumma");
        assert_eq!(canonical_irc_nick("kukakumma"), "kukakumma");
        assert_eq!(canonical_irc_nick("ra-"), "ra");
        assert_eq!(canonical_irc_nick("Maska"), "maska");
        assert_eq!(canonical_irc_nick("nick|afk"), "nick");
        assert_eq!(canonical_irc_nick("nick[away]"), "nick");
        // A two-part nick is not over-merged.
        assert_eq!(canonical_irc_nick("veli-v"), "veli-v");
        assert_eq!(canonical_irc_nick("kuka_kumma"), "kuka_kumma");
    }
}
