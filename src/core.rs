//! Platform-agnostic helpers shared by both bots: parsing the `!top` / `/top`
//! argument, formatting the leaderboard, and the funny title list for `naamat`.

use crate::db::{LeaderEntry, Scope};
use rand::seq::SliceRandom;

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
    "Kippis! 🍺",
    "Skål! 🍺",
    "Cheers! 🍺",
    "Prost! 🍺",
    "Santé! 🍺",
    "Salud! 🍺",
    "Salute! 🍺",
    "Na zdrowie! 🍺",
    "Na zdorovie! 🍺",
    "Kanpai! 🍺",
    "Gānbēi! 🍺",
    "Sláinte! 🍺",
    "Proost! 🍺",
    "Saúde! 🍺",
    "Yamas! 🍺",
    "Şerefe! 🍺",
    "Egészségedre! 🍺",
    "Noroc! 🍺",
];

pub fn random_cheers() -> &'static str {
    CHEERS
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or("Kippis! 🍺")
}

/// Encouraging words for `!nousuun` / `/nousuun` — cheering on the weary as they
/// try to drink their way out of the hangover (nousuhumala).
pub const NOUSUUN_WORDS: &[&str] = &[
    "Yksi kalja vielä, niin nousuhumala iskee! Sisua peliin! 💪🍺",
    "Krapula on vain heikkojen tila. Nouse ja juo, sankari! 🦸",
    "Klaara lasi, niin maailma kirkastuu taas. Sinä pystyt tähän! 🌅",
    "Älä anna periksi — paras humala on nousuhumala. Kohti uusia seikkailuja! 🚀",
    "Pää kipeä? Lääke on tunnettu. Pohjat ja nousuun! 🍻",
    "Eilinen oli kova, mutta sinä olet kovempi. Yksi vielä ja lentoon! ✈️",
    "Vesilasi on petturi. Tartu kaljaan ja nouse tuhkasta kuin feeniks! 🔥",
    "Nousuhumala kutsuu. Vastaa rohkeasti — krapula kumartaa pian! 👑",
];

pub fn random_nousuun() -> &'static str {
    NOUSUUN_WORDS
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or("Pohjat ja nousuun! 🍻")
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
        return format!("{header}: ei yhtään krappea vielä. Hienoa työtä! 🎉");
    }
    let mut out = format!("{header}:\n");
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&format!("{}. {} — {}\n", i + 1, e.display, e.count));
    }
    out.trim_end().to_string()
}

pub fn scope_header(scope: Scope) -> String {
    match scope {
        Scope::Year => format!("Krappe-tilasto {}", chrono::Utc::now().format("%Y")),
        Scope::All => "Krappe-tilasto (kaikkien aikojen)".to_string(),
    }
}
