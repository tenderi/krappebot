//! SQLite storage layer: recording krappe events, building the leaderboard,
//! and linking a Telegram user to an IRC nick (`/combine`).
//!
//! Uses the runtime `sqlx::query` API (not the `query!` macro) so no live
//! database is needed at compile time.

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

pub const PLATFORM_IRC: &str = "irc";
pub const PLATFORM_TELEGRAM: &str = "telegram";

/// Scope for the leaderboard.
#[derive(Clone, Copy, Debug)]
pub enum Scope {
    /// Current calendar year only.
    Year,
    /// All-time.
    All,
}

/// One row of the leaderboard.
#[derive(Clone, Debug)]
pub struct LeaderEntry {
    pub display: String,
    pub count: i64,
}

/// Open (creating if needed) the SQLite pool and run migrations.
pub async fn init(database_url: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Record one krappe. `user_key` should be the lowercased IRC nick or the
/// Telegram user id; `display_name` is what we show on the leaderboard.
/// Returns the canonical leaderboard count for that identity after inserting.
pub async fn record_krappe(
    pool: &SqlitePool,
    platform: &str,
    user_key: &str,
    display_name: &str,
) -> Result<i64> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO events (platform, user_key, display_name, created_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(platform)
    .bind(user_key)
    .bind(display_name)
    .bind(&now)
    .execute(pool)
    .await?;

    count_for(pool, platform, user_key).await
}

/// Outcome of an at-most-once-per-day krappe attempt.
pub enum KrappeOutcome {
    /// First krappe today; recorded. Carries the new total.
    Recorded(i64),
    /// Already krappe'd today; nothing recorded. Carries the existing total.
    AlreadyToday(i64),
}

/// Record a krappe only if this (platform, user_key) has none yet today (UTC).
/// A second attempt the same day is rejected so it can be shamed instead.
pub async fn record_krappe_daily(
    pool: &SqlitePool,
    platform: &str,
    user_key: &str,
    display_name: &str,
) -> Result<KrappeOutcome> {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events
         WHERE platform = ? AND user_key = ? AND substr(created_at, 1, 10) = ?",
    )
    .bind(platform)
    .bind(user_key)
    .bind(&today)
    .fetch_one(pool)
    .await?;

    if existing > 0 {
        return Ok(KrappeOutcome::AlreadyToday(
            count_for(pool, platform, user_key).await?,
        ));
    }
    Ok(KrappeOutcome::Recorded(
        record_krappe(pool, platform, user_key, display_name).await?,
    ))
}

/// All-time count for the canonical identity behind a single (platform, user_key).
async fn count_for(pool: &SqlitePool, platform: &str, user_key: &str) -> Result<i64> {
    // Resolve this event's canonical key, then count everything sharing it.
    let canon = canonical_key(pool, platform, user_key).await?;
    let row = sqlx::query(
        "SELECT COUNT(*) AS c FROM events e
         LEFT JOIN links l
           ON e.platform = 'telegram' AND e.user_key = l.telegram_id
         WHERE COALESCE(
                 l.irc_nick,
                 CASE WHEN e.platform = 'telegram' THEN 'tg:' || e.user_key ELSE e.user_key END
               ) = ?",
    )
    .bind(&canon)
    .fetch_one(pool)
    .await?;
    Ok(row.get::<i64, _>("c"))
}

/// Compute the canonical key for a (platform, user_key) pair, honoring /combine links.
async fn canonical_key(pool: &SqlitePool, platform: &str, user_key: &str) -> Result<String> {
    if platform == PLATFORM_TELEGRAM {
        if let Some(row) = sqlx::query("SELECT irc_nick FROM links WHERE telegram_id = ?")
            .bind(user_key)
            .fetch_optional(pool)
            .await?
        {
            return Ok(row.get::<String, _>("irc_nick"));
        }
        return Ok(format!("tg:{user_key}"));
    }
    Ok(user_key.to_string())
}

/// Build the leaderboard. Identities are merged by canonical key; for each we take
/// the most recent display name. IRC-linked identities display as the IRC nick.
pub async fn leaderboard(pool: &SqlitePool, scope: Scope, limit: i64) -> Result<Vec<LeaderEntry>> {
    let since: Option<String> = match scope {
        Scope::All => None,
        Scope::Year => Some(format!("{}-01-01T00:00:00Z", Utc::now().format("%Y"))),
    };

    // Window functions resolve, per canonical key, the total count and the
    // display_name from the most recent event (rn = 1).
    let rows = sqlx::query(
        "SELECT canon, display_name, cnt FROM (
            SELECT
                COALESCE(
                    l.irc_nick,
                    CASE WHEN e.platform = 'telegram' THEN 'tg:' || e.user_key ELSE e.user_key END
                ) AS canon,
                e.display_name AS display_name,
                COUNT(*)     OVER (PARTITION BY COALESCE(
                    l.irc_nick,
                    CASE WHEN e.platform = 'telegram' THEN 'tg:' || e.user_key ELSE e.user_key END
                )) AS cnt,
                ROW_NUMBER() OVER (PARTITION BY COALESCE(
                    l.irc_nick,
                    CASE WHEN e.platform = 'telegram' THEN 'tg:' || e.user_key ELSE e.user_key END
                ) ORDER BY e.created_at DESC) AS rn
            FROM events e
            LEFT JOIN links l
              ON e.platform = 'telegram' AND e.user_key = l.telegram_id
            WHERE (?1 IS NULL OR e.created_at >= ?1)
        ) WHERE rn = 1
        ORDER BY cnt DESC, canon ASC
        LIMIT ?2",
    )
    .bind(since)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let canon: String = r.get("canon");
            let display_name: String = r.get("display_name");
            // IRC nicks (native or linked) display as the nick; bare Telegram users
            // (no /combine) display their Telegram name.
            let display = if canon.starts_with("tg:") {
                display_name
            } else {
                canon
            };
            LeaderEntry {
                display,
                count: r.get::<i64, _>("cnt"),
            }
        })
        .collect())
}

/// Krappe count, rank, and field size for one nick within a scope.
pub struct ScopeStats {
    pub count: i64,
    pub rank: i64,
    pub people: i64,
}

/// Stats for a single canonical identity, as shown by `!stat` / `/stat`:
/// the current year (primary) plus all-time totals.
pub struct NickStats {
    pub name: String,
    pub year: ScopeStats,
    pub all: ScopeStats,
}

// Canonical-identity expression and join, shared by the stats queries (matches
// the grouping used in `leaderboard`).
const CANON_EXPR: &str =
    "COALESCE(l.irc_nick, CASE WHEN e.platform = 'telegram' THEN 'tg:' || e.user_key ELSE e.user_key END)";
const CANON_JOIN: &str =
    "FROM events e LEFT JOIN links l ON e.platform = 'telegram' AND e.user_key = l.telegram_id";

/// Count, rank and field size for `canon` within a scope. `since` is an ISO
/// timestamp lower bound, or "" for all-time. Rank counts identities with a
/// strictly higher count in the same scope; if the nick has 0 in scope its rank
/// is meaningless (callers gate on `count`).
async fn scope_stats(pool: &SqlitePool, canon: &str, since: &str) -> Result<ScopeStats> {
    let row = sqlx::query(&format!(
        "WITH counts AS (
            SELECT {CANON_EXPR} AS canon, COUNT(*) AS c
            {CANON_JOIN}
            WHERE (?1 = '' OR e.created_at >= ?1)
            GROUP BY canon
         )
         SELECT
            COALESCE((SELECT c FROM counts WHERE canon = ?2), 0) AS count,
            (SELECT COUNT(*) FROM counts
                WHERE c > COALESCE((SELECT c FROM counts WHERE canon = ?2), 0)) + 1 AS rank,
            (SELECT COUNT(*) FROM counts) AS people"
    ))
    .bind(since)
    .bind(canon)
    .fetch_one(pool)
    .await?;

    Ok(ScopeStats {
        count: row.get("count"),
        rank: row.get("rank"),
        people: row.get("people"),
    })
}

/// Current-year and all-time stats for one nick. `None` if the nick has no
/// krappe ever.
pub async fn nick_stats(pool: &SqlitePool, canon: &str) -> Result<Option<NickStats>> {
    let all = scope_stats(pool, canon, "").await?;
    if all.count == 0 {
        return Ok(None);
    }
    let since = format!("{}-01-01T00:00:00Z", Utc::now().format("%Y"));
    let year = scope_stats(pool, canon, &since).await?;
    Ok(Some(NickStats {
        name: canon.to_string(),
        year,
        all,
    }))
}

/// Per-year krappe counts for one nick, oldest year first. Empty if never seen.
pub async fn nick_yearly(pool: &SqlitePool, canon: &str) -> Result<Vec<(i32, i64)>> {
    let rows = sqlx::query(&format!(
        "SELECT substr(e.created_at, 1, 4) AS yr, COUNT(*) AS c
         {CANON_JOIN}
         WHERE {CANON_EXPR} = ?
         GROUP BY yr
         ORDER BY yr"
    ))
    .bind(canon)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let yr: String = r.get("yr");
            yr.parse::<i32>().ok().map(|y| (y, r.get::<i64, _>("c")))
        })
        .collect())
}

/// Tie a Telegram user id to an IRC nick (last writer wins).
pub async fn link_combine(pool: &SqlitePool, telegram_id: &str, irc_nick: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let nick = irc_nick.to_lowercase();
    sqlx::query(
        "INSERT INTO links (telegram_id, irc_nick, created_at) VALUES (?, ?, ?)
         ON CONFLICT(telegram_id) DO UPDATE SET irc_nick = excluded.irc_nick,
                                                created_at = excluded.created_at",
    )
    .bind(telegram_id)
    .bind(&nick)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn mem_pool() -> SqlitePool {
        // Each :memory: pool is isolated; run migrations into it.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// A /combine link merges an IRC nick and a Telegram id into one leaderboard row.
    #[tokio::test]
    async fn combine_merges_counts() {
        let pool = mem_pool().await;

        // Two krappe on IRC as "helge".
        record_krappe(&pool, PLATFORM_IRC, "helge", "helge").await.unwrap();
        record_krappe(&pool, PLATFORM_IRC, "helge", "helge").await.unwrap();

        // One krappe on Telegram as id 42, not yet linked -> separate row.
        record_krappe(&pool, PLATFORM_TELEGRAM, "42", "@helge_tg").await.unwrap();

        let before = leaderboard(&pool, Scope::All, 10).await.unwrap();
        assert_eq!(before.len(), 2, "unlinked telegram user is its own row");

        // Link telegram 42 -> helge, add one more telegram krappe.
        link_combine(&pool, "42", "Helge").await.unwrap();
        let count = record_krappe(&pool, PLATFORM_TELEGRAM, "42", "@helge_tg").await.unwrap();
        assert_eq!(count, 4, "2 irc + 2 telegram now merge under helge");

        let after = leaderboard(&pool, Scope::All, 10).await.unwrap();
        assert_eq!(after.len(), 1, "linked users collapse into one row");
        assert_eq!(after[0].display, "helge");
        assert_eq!(after[0].count, 4);
    }

    /// Year scope excludes events from previous years; All-time includes them.
    #[tokio::test]
    async fn year_scope_filters_old_events() {
        let pool = mem_pool().await;

        // Backdate one event into a prior year by inserting directly.
        sqlx::query(
            "INSERT INTO events (platform, user_key, display_name, created_at)
             VALUES ('irc', 'old', 'old', '2000-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        record_krappe(&pool, PLATFORM_IRC, "now", "now").await.unwrap();

        let all = leaderboard(&pool, Scope::All, 10).await.unwrap();
        assert_eq!(all.len(), 2, "all-time sees the year-2000 event");

        let year = leaderboard(&pool, Scope::Year, 10).await.unwrap();
        assert_eq!(year.len(), 1, "current-year scope hides the old event");
        assert_eq!(year[0].display, "now");
    }

    /// A second krappe on the same day is rejected; the count stays put.
    #[tokio::test]
    async fn daily_dedup_blocks_second_krappe() {
        let pool = mem_pool().await;

        match record_krappe_daily(&pool, PLATFORM_IRC, "helge", "helge").await.unwrap() {
            KrappeOutcome::Recorded(1) => {}
            _ => panic!("first krappe of the day should be recorded with count 1"),
        }
        match record_krappe_daily(&pool, PLATFORM_IRC, "helge", "helge").await.unwrap() {
            KrappeOutcome::AlreadyToday(1) => {}
            _ => panic!("second krappe same day should be rejected, count still 1"),
        }

        let board = leaderboard(&pool, Scope::All, 10).await.unwrap();
        assert_eq!(board.len(), 1);
        assert_eq!(board[0].count, 1, "only one krappe counted for the day");
    }

    /// nick_stats reports totals and rank, and None for an unknown nick.
    #[tokio::test]
    async fn nick_stats_reports_total_and_rank() {
        let pool = mem_pool().await;
        for _ in 0..3 {
            sqlx::query(
                "INSERT INTO events (platform, user_key, display_name, created_at)
                 VALUES ('irc', 'maska', 'maska', '2020-01-01T00:00:00Z')",
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        record_krappe(&pool, PLATFORM_IRC, "spott", "spott").await.unwrap();

        let maska = nick_stats(&pool, "maska").await.unwrap().unwrap();
        assert_eq!(maska.all.count, 3);
        assert_eq!(maska.all.rank, 1);
        assert_eq!(maska.all.people, 2);
        assert_eq!(maska.year.count, 0, "maska's krappe were in 2020, not this year");

        let spott = nick_stats(&pool, "spott").await.unwrap().unwrap();
        assert_eq!(spott.all.rank, 2, "fewer krappe -> lower rank");
        assert_eq!(spott.year.count, 1, "spott krappe'd this year");

        assert_eq!(nick_yearly(&pool, "maska").await.unwrap(), vec![(2020, 3)]);
        assert!(nick_stats(&pool, "nobody").await.unwrap().is_none());
    }
}
