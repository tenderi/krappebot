//! One-off importer: rebuild historical IRC krappe events from two sources —
//! the krappe.fi/history/ yearly archive (authoritative wherever it has a
//! year file) and the irssi channel log (used only for years the archive
//! doesn't cover, i.e. the current year(s) since the archive was retired).
//!
//! Usage:  cargo run --release --bin import -- [path-to-log] [archive-dir]
//! Defaults: log "#tty-krappe.log", archive dir "history" (containing files
//! like "2010.txt".."2024.txt", one per year, each line "<count> <nick>").
//! DB comes from DATABASE_URL (default sqlite://krappe.db), same as the bot.
//!
//! Source-of-truth rule: a year present in the archive dir is taken entirely
//! from the archive (the archive's own counting rules — not necessarily the
//! same once-per-day dedup the bot uses — apply for that year); any other
//! year is parsed from the log with the live bot's exact rules:
//!   * a krappe is a message whose first whitespace token is exactly "!krappe";
//!   * the log owner's own messages are logged as "HH:MM > ..." with no nick, and
//!     are attributed to OWNER ("tenderi");
//!   * alt-nicks are canonicalized (see core::canonical_irc_nick);
//!   * at most one krappe per (canonical nick, day) is counted.
//!
//! It is idempotent: it deletes all existing `platform = 'irc'` events and
//! re-inserts everything parsed from both sources. Telegram events are untouched.

use anyhow::{Context, Result};
use krappebot::core::canonical_irc_nick;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;

/// The log belongs to this user; their own messages have no nick in the log.
const OWNER: &str = "tenderi";

struct Event {
    timestamp: String, // ISO-8601, e.g. 2015-03-04T10:33:00Z
    user_key: String,  // canonical nick
    display: String,   // nick as seen in the log
}

fn month_num(name: &str) -> Option<u32> {
    Some(match name {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

/// (year, month, day) from an irssi date directive, whitespace-tolerant.
/// `Log opened` tokens: Thu Sep 25 10:30:21 2014  -> year is tokens[4]
/// `Day changed` tokens: Fri Sep 26 2014          -> year is tokens[3]
fn parse_date(tokens: &[&str]) -> Option<(i32, u32, u32)> {
    let month = month_num(tokens.get(1)?)?;
    let day: u32 = tokens.get(2)?.parse().ok()?;
    let year_tok = if tokens.len() >= 5 { tokens[4] } else { tokens.get(3)? };
    let year: i32 = year_tok.parse().ok()?;
    Some((year, month, day))
}

/// Parse a chat line into (hour, minute, nick, message). Handles both a normal
/// `HH:MM <[sigil]nick> message` and the owner's `HH:MM > message`.
fn parse_line(line: &str) -> Option<(u32, u32, String, &str)> {
    let b = line.as_bytes();
    if b.len() < 7 || b[2] != b':' || b[5] != b' ' {
        return None;
    }
    let hh: u32 = line.get(0..2)?.parse().ok()?;
    let mm: u32 = line.get(3..5)?.parse().ok()?;
    let rest = &line[6..];

    if let Some(r) = rest.strip_prefix('<') {
        // Normal message from another user.
        let close = r.find('>')?;
        let nick = r[..close].trim_start_matches([' ', '@', '+', '%', '&', '~']);
        if nick.is_empty() {
            return None;
        }
        Some((hh, mm, nick.to_string(), r[close + 1..].trim_start()))
    } else if let Some(r) = rest.strip_prefix("> ") {
        // Owner's own message — no nick in the log.
        Some((hh, mm, OWNER.to_string(), r.trim_start()))
    } else {
        None
    }
}

/// Parse one archive year file: lines "<count> <nick>" (whitespace-separated,
/// anything else — like krappe.fi's odd header line on 2010.txt — is skipped).
/// Emits `count` events per nick, timestamped mid-year since the archive has
/// no per-event dates; only the year is ever queried, so the exact day/time
/// within it doesn't matter.
fn parse_archive_year(path: &std::path::Path, year: i32) -> Result<Vec<Event>> {
    let file = File::open(path).with_context(|| format!("opening archive file {path:?}"))?;
    let reader = BufReader::new(file);
    let timestamp = format!("{year:04}-07-02T12:00:00Z");

    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let mut words = line.split_whitespace();
        let (Some(count_tok), Some(nick), None) = (words.next(), words.next(), words.next())
        else {
            continue; // not a "<count> <nick>" line (e.g. the 2010.txt header)
        };
        let Ok(count) = count_tok.parse::<u32>() else { continue };
        let key = canonical_irc_nick(nick);
        for _ in 0..count {
            events.push(Event {
                timestamp: timestamp.clone(),
                user_key: key.clone(),
                display: nick.to_string(),
            });
        }
    }
    Ok(events)
}

/// Read `dir` for files named "<year>.txt" and parse each into events. Returns
/// the events plus the set of years the archive covers (so the log parser can
/// skip them). Missing directory -> no archive years, log-only (unchanged
/// behavior).
fn load_archive(dir: &std::path::Path) -> Result<(Vec<Event>, HashSet<i32>)> {
    let mut events = Vec::new();
    let mut years = HashSet::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((events, years)),
        Err(e) => return Err(e).with_context(|| format!("reading archive dir {dir:?}")),
    };
    for entry in entries {
        let path = entry?.path();
        let Some(year) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue; // not a "<year>.txt" file
        };
        events.extend(parse_archive_year(&path, year)?);
        years.insert(year);
    }
    Ok((events, years))
}

async fn connect(database_url: &str) -> Result<sqlx::SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let log_path = std::env::args().nth(1).unwrap_or_else(|| "#tty-krappe.log".to_string());
    let archive_dir = std::env::args().nth(2).unwrap_or_else(|| "history".to_string());
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://krappe.db".to_string());

    let (mut events, archive_years) = load_archive(std::path::Path::new(&archive_dir))?;
    let archive_count = events.len();
    println!(
        "{archive_count} events from {} archive year(s) in {archive_dir}",
        archive_years.len()
    );

    let file = File::open(&log_path).with_context(|| format!("opening log {log_path}"))?;
    let reader = BufReader::new(file);

    let mut current: Option<(i32, u32, u32)> = None;
    let mut log_events: Vec<Event> = Vec::new();
    let mut seen: HashSet<(String, (i32, u32, u32))> = HashSet::new(); // (canonical nick, day)
    let mut lines = 0u64;
    let mut raw_hits = 0u64; // total !krappe invocations before daily dedup, in non-archive years

    for line in reader.lines() {
        let line = line?;
        lines += 1;

        if let Some(rest) = line.strip_prefix("--- ") {
            if let Some(after) = rest
                .strip_prefix("Log opened ")
                .or_else(|| rest.strip_prefix("Day changed "))
            {
                let tokens: Vec<&str> = after.split_whitespace().collect();
                if let Some(date) = parse_date(&tokens) {
                    current = Some(date);
                }
            }
            continue;
        }

        let Some((hh, mm, nick, message)) = parse_line(&line) else {
            continue;
        };
        if message.split_whitespace().next() != Some("!krappe") {
            continue;
        }
        let Some(day) = current else { continue };
        // The archive is the source of truth for any year it covers; only use
        // the log to fill in years the archive doesn't have.
        if archive_years.contains(&day.0) {
            continue;
        }
        raw_hits += 1;

        let key = canonical_irc_nick(&nick);
        // At most one krappe per person per day.
        if !seen.insert((key.clone(), day)) {
            continue;
        }
        let (y, mo, d) = day;
        log_events.push(Event {
            timestamp: format!("{y:04}-{mo:02}-{d:02}T{hh:02}:{mm:02}:00Z"),
            user_key: key,
            display: nick,
        });
    }

    println!("Scanned {lines} lines from {log_path}");
    println!("{raw_hits} raw !krappe invocations outside archive years");
    println!(
        "{} counted after once-per-day dedup, from {} people",
        log_events.len(),
        log_events.iter().map(|e| &e.user_key).collect::<HashSet<_>>().len()
    );

    events.extend(log_events);

    // Overall summary across both sources.
    let mut by_nick: HashMap<String, u64> = HashMap::new();
    for e in &events {
        *by_nick.entry(e.user_key.clone()).or_insert(0) += 1;
    }
    let mut top: Vec<(&String, &u64)> = by_nick.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));

    let first = events.iter().map(|e| &e.timestamp).min().cloned().unwrap_or_default();
    let last = events.iter().map(|e| &e.timestamp).max().cloned().unwrap_or_default();

    println!(
        "Combined: {} events from {} people ({}..{})",
        events.len(),
        by_nick.len(),
        first,
        last
    );
    println!("Top 20:");
    for (nick, count) in top.iter().take(20) {
        println!("  {count:>5}  {nick}");
    }

    // Rebuild IRC events transactionally.
    let pool = connect(&database_url).await?;
    let mut tx = pool.begin().await?;
    let deleted = sqlx::query("DELETE FROM events WHERE platform = 'irc'")
        .execute(&mut *tx)
        .await?
        .rows_affected();
    for e in &events {
        sqlx::query(
            "INSERT INTO events (platform, user_key, display_name, created_at)
             VALUES ('irc', ?, ?, ?)",
        )
        .bind(&e.user_key)
        .bind(&e.display)
        .bind(&e.timestamp)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    println!(
        "Done: deleted {deleted} old IRC events, inserted {} ({archive_count} archive + {} log) into {database_url}",
        events.len(),
        events.len() - archive_count
    );
    Ok(())
}
