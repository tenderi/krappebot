-- Each !krappe / /krappe use is one event row. Counts are aggregates over events,
-- which lets us scope by year or all-time without resetting anything.
CREATE TABLE IF NOT EXISTS events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    platform     TEXT NOT NULL,           -- 'irc' | 'telegram'
    user_key     TEXT NOT NULL,           -- lowercased irc nick, or telegram user id
    display_name TEXT NOT NULL,           -- nick or @username, shown on the leaderboard
    created_at   TEXT NOT NULL            -- ISO-8601 UTC, e.g. 2026-06-20T12:00:00Z
);

CREATE INDEX IF NOT EXISTS idx_events_created_at ON events(created_at);

-- A Telegram user can run /combine <irc nick> to merge their counts under that nick.
CREATE TABLE IF NOT EXISTS links (
    telegram_id  TEXT PRIMARY KEY,        -- telegram user id (as text)
    irc_nick     TEXT NOT NULL,           -- lowercased target irc nick
    created_at   TEXT NOT NULL
);
