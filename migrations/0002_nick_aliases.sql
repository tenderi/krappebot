-- Manual multi-nick merges (`!combine` on IRC, or a second/third `/combine` on
-- Telegram): folds `alias_nick`'s events into whatever `canonical_nick` already
-- resolves to. Kept single-hop (never chained) by add_nick_alias in db.rs.
CREATE TABLE IF NOT EXISTS nick_aliases (
    alias_nick     TEXT PRIMARY KEY,  -- canonicalized nick being folded in
    canonical_nick TEXT NOT NULL,     -- canonicalized nick it now counts under
    created_at     TEXT NOT NULL
);
