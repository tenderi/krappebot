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
}
