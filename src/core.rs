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
