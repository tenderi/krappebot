//! Platform-agnostic helpers shared by both bots and the importer: nick
//! canonicalization, command-argument parsing, leaderboard formatting, and the
//! various random phrase lists. Per project rule, none of these strings contain
//! emojis.

use crate::db::{LeaderEntry, NickStats, Scope};
use rand::seq::SliceRandom;

/// Real IRC nicks are capped server-side (RFC 2812 suggests >= 9 chars; most
/// modern networks allow up to ~30). This is a generous ceiling on the
/// canonicalized result so free-text arguments to `!stat`/`!combine`/
/// `!uncombine` (and their Telegram equivalents) can't make the bot echo an
/// arbitrarily long string back into a reply — see IRC_SAFE_LINE_BYTES, the
/// same RFC 512-byte-line concern applied to a different code path.
const MAX_NICK_CHARS: usize = 32;

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
    let base = if base.is_empty() { lower.as_str() } else { base };
    base.chars().take(MAX_NICK_CHARS).collect()
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

/// Parse `!stat`/`/stat` arguments into an optional nick and the "all" flag.
/// No words -> no nick, current-year scope. A lone "all" -> no nick, all-time
/// scope (i.e. the requester's own per-year breakdown). Otherwise the first
/// word is the nick and a following "all" (case-insensitive) selects all-time.
/// A missing nick means "default to whoever asked" — callers fill that in.
pub fn parse_stat_args<'a>(mut words: impl Iterator<Item = &'a str>) -> (Option<&'a str>, bool) {
    match words.next() {
        None => (None, false),
        Some(first) if first.eq_ignore_ascii_case("all") => (None, true),
        Some(nick) => {
            let all = words.next().is_some_and(|w| w.eq_ignore_ascii_case("all"));
            (Some(nick), all)
        }
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

/// Conservative budget (bytes) for the text of a single IRC NOTICE/PRIVMSG.
/// RFC 2812 hard-caps a full line — including the server's relayed
/// `:nick!user@host ` prefix and the trailing CRLF, neither of which we know
/// the size of here — at 512 bytes total, so this leaves generous headroom
/// rather than risk the server silently truncating what other clients see.
const IRC_SAFE_LINE_BYTES: usize = 400;

/// Like [`format_leaderboard`] but on a single line (entries joined by ", "),
/// for IRC where one NOTICE cannot contain newlines. Stops adding entries
/// before the line would risk exceeding IRC's line-length limit, noting how
/// many were left off rather than letting the server truncate mid-entry.
pub fn format_leaderboard_inline(header: &str, entries: &[LeaderEntry]) -> String {
    if entries.is_empty() {
        return format!("{header}: ei yhtään krappea vielä. Hienoa työtä!");
    }
    let mut out = format!("{header}: ");
    let mut shown = 0;
    for (i, e) in entries.iter().enumerate() {
        let piece = format!("{}. {} - {}", i + 1, e.display, e.count);
        let sep = if shown == 0 { "" } else { ", " };
        // Always include at least one entry, even if it alone is oversized.
        if shown > 0 && out.len() + sep.len() + piece.len() > IRC_SAFE_LINE_BYTES {
            break;
        }
        out.push_str(sep);
        out.push_str(&piece);
        shown += 1;
    }
    if shown < entries.len() {
        out.push_str(&format!(" (+{} lisää)", entries.len() - shown));
    }
    out
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

/// Build the `!stat` / `/stat` reply. `canon` is the DB lookup key; `display` is
/// the name shown in the reply (usually the same string, but a Telegram user's
/// own unlinked lookup shows their Telegram display name instead of `tg:<id>`).
/// `all` selects the per-year breakdown; otherwise the current-year-primary
/// summary. Shared by both bots so the wording stays identical.
pub async fn stat_reply(pool: &sqlx::SqlitePool, canon: &str, display: &str, all: bool) -> String {
    if all {
        match crate::db::nick_yearly(pool, canon).await {
            Ok(yearly) => format_nick_yearly(display, &yearly),
            Err(e) => {
                tracing::error!(error = %e, "nick_yearly failed");
                "Tilaston haku epäonnistui.".to_string()
            }
        }
    } else {
        match crate::db::nick_stats(pool, canon).await {
            Ok(Some(mut stats)) => {
                stats.name = display.to_string();
                format_nick_stats(&stats)
            }
            Ok(None) => format!("{display}: ei yhtään krappea."),
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
    use super::{canonical_irc_nick, format_leaderboard_inline, parse_stat_args, IRC_SAFE_LINE_BYTES};
    use crate::db::LeaderEntry;

    #[test]
    fn leaderboard_inline_stays_under_the_irc_line_budget() {
        // 200 long-ish entries would be several kB unabridged -- comfortably
        // over the budget, so this must get cut off well short of all of them.
        let entries: Vec<LeaderEntry> = (0..200)
            .map(|i| LeaderEntry { display: format!("someone-called-person-{i}"), count: 999 })
            .collect();
        let out = format_leaderboard_inline("Krappe-tilasto", &entries);
        assert!(
            out.len() <= IRC_SAFE_LINE_BYTES + 32,
            "line ({} bytes) should stay near the safety budget even with many entries",
            out.len()
        );
        assert!(out.contains("lisää"), "truncated output should say how many were left off");
    }

    #[test]
    fn leaderboard_inline_keeps_a_lone_oversized_entry() {
        // Even a single entry that alone exceeds the budget must still be shown --
        // never produce an empty line.
        let entries = vec![LeaderEntry { display: "x".repeat(1000), count: 1 }];
        let out = format_leaderboard_inline("Krappe-tilasto", &entries);
        assert!(out.contains("1. "), "the one entry should still be included");
    }

    #[test]
    fn parses_stat_args() {
        assert_eq!(parse_stat_args("".split_whitespace()), (None, false));
        assert_eq!(parse_stat_args("all".split_whitespace()), (None, true));
        assert_eq!(parse_stat_args("ALL".split_whitespace()), (None, true));
        assert_eq!(parse_stat_args("helge".split_whitespace()), (Some("helge"), false));
        assert_eq!(
            parse_stat_args("helge all".split_whitespace()),
            (Some("helge"), true)
        );
        assert_eq!(
            parse_stat_args("helge ALL".split_whitespace()),
            (Some("helge"), true)
        );
        assert_eq!(
            parse_stat_args("helge nope".split_whitespace()),
            (Some("helge"), false)
        );
    }

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

    /// A wildly long `!stat`/`!combine` argument can't make the bot echo an
    /// unbounded string back into an IRC reply.
    #[test]
    fn canonical_irc_nick_caps_length() {
        let long = "a".repeat(500);
        assert_eq!(canonical_irc_nick(&long).chars().count(), 32);

        // Also bounded via the empty-after-stripping fallback path.
        let long_underscores = "_".repeat(500);
        assert_eq!(canonical_irc_nick(&long_underscores).chars().count(), 32);
    }
}
