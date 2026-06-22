# krappebot

A bot that counts **krappe** (hangovers) for a friend group across both **IRC** and
**Telegram**, with a yearly/all-time leaderboard. One Rust binary runs both bots as
concurrent async tasks sharing a single SQLite database.

## Commands

| Command | IRC | Telegram |
| --- | --- | --- |
| `!krappe` / `/krappe` | +1 krappe and gives you **+v** | +1 krappe |
| `!naamat` / `/naamat` | gives you **+o** | promotes you and sets a funny admin title (e.g. `KRAPULA`) |
| `!top` / `/top` | this year's leaderboard (top 20) | this year's leaderboard (top 20) |
| `!top all` / `/top all` | all-time leaderboard (top 20) | all-time leaderboard (top 20) |
| `!stat <nick>` / `/stat <nick>` | one nick's totals + rank | one nick's totals + rank |
| `!kalja` / `/kalja` | "Cheers!" in a random language | "Cheers!" in a random language |
| `!nousuun` / `/nousuun` | encouraging words for the hungover | encouraging words for the hungover |
| — | — | `/combine <irc nick>` — tie your Telegram account to an IRC nick so the counts merge |

## How identities work

Counts are stored per event. IRC events are keyed by nick; Telegram events by user id.
A Telegram user who runs `/combine <irc nick>` has their krappe merged under that IRC nick
on the leaderboard. Without `/combine`, IRC and Telegram totals stay separate.

Alt-nicks are merged automatically: a nick is canonicalized by lowercasing, dropping any
`|`/`[` away-suffix, and stripping trailing reconnect markers (`_ - \``), so `Kukakumma_`
and `kukakumma` count as one person.

## Once per day

`!krappe` / `/krappe` counts **at most once per person per calendar day** — a hangover is a
hangover. A second attempt the same day is not counted and earns a gentle shaming reply.

## Importing historical IRC logs

To backfill stats from an irssi channel log, point the importer at it:

```bash
DATABASE_URL=sqlite://krappe.db cargo run --release --bin import -- "#tty-krappe.log"
```

It parses `!krappe` lines (including the log owner's own nick-less `>` lines, attributed to
`tenderi`), applies the same canonicalization and once-per-day rule, and **replaces** all
existing IRC events with what it parsed. Telegram events are left untouched. Stop the bot
first so the database isn't being written concurrently. Logs are gitignored — never commit them.

## Setup

1. Install Rust (see below) and the C build tools.
2. `cp .env.example .env` and fill in your tokens/server (see comments in that file).
3. `cargo run`

### Requirements / permissions

- **Telegram:** create a bot with [@BotFather](https://t.me/BotFather). For `/naamat` to set
  a custom admin title, add the bot to the group **as an admin** with the "Add new admins"
  right. If it lacks that right it falls back to a text-only "wasted" message. Note Telegram
  only lets a bot title admins it promoted, and titles are capped at 16 characters.
- **IRC:** the bot must be **opped** in the channel to grant `+v` / `+o`. Set
  `IRC_NICKSERV_PASSWORD` if the nick is registered.

## Installing the toolchain (Arch Linux)

```bash
sudo pacman -S --needed rustup base-devel pkgconf sqlite
rustup default stable
```

(TLS uses rustls, so OpenSSL is not required.)

## Development

```bash
cargo build      # compile
cargo test       # runs the db.rs unit tests against an in-memory SQLite
cargo run        # start the bot(s) configured in .env
```

## Layout

```
src/
  main.rs          runtime: load config, init db, spawn both bots
  config.rs        env-based configuration
  db.rs            SQLite: record_krappe, leaderboard, link_combine (+ tests)
  core.rs          shared formatting, scope parsing, naamat titles
  telegram_bot.rs  teloxide commands
  irc_bot.rs       irc crate client + MODE handling
migrations/
  0001_init.sql    events + links tables
```
