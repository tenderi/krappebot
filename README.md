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
| `!stat [nick]` / `/stat [nick]` | nick's current-year count + rank (and all-time); omit the nick for your own stats | same |
| `!stat [nick] all` / `/stat [nick] all` | nick's per-year breakdown; omit the nick for your own | same |
| `!kalja` / `/kalja` | "Cheers!" in a random language | "Cheers!" in a random language |
| `!nousuun` / `/nousuun` | encouraging words for the hungover | encouraging words for the hungover |
| `!combine <nick>` | merges `<nick>`'s krappe into yours | `/combine <nick>` — ties your Telegram account (and any nick already merged into it) to an IRC nick so the counts merge |

Both `!combine` and `/combine` are repeatable: running them again with another nick adds
it to the same identity rather than replacing the link, so someone who used several IRC
nicks over the years can fold them all together.

## How identities work

Counts are stored per event. IRC events are keyed by nick; Telegram events by user id.
A Telegram user who runs `/combine <irc nick>` has their krappe merged under that IRC nick
on the leaderboard. Without `/combine`, IRC and Telegram totals stay separate.

Alt-nicks are merged automatically: a nick is canonicalized by lowercasing, dropping any
`|`/`[` away-suffix, and stripping trailing reconnect markers (`_ - \``), so `Kukakumma_`
and `kukakumma` count as one person. This only catches spelling variants of the *same*
nick, though — someone who genuinely changed nicks (e.g. `Veli-V` → `Veli`) needs an
explicit `!combine`/`/combine` to fold the old one in, since nothing else can tell those
apart from two different people.

## Once per day

`!krappe` / `/krappe` counts **at most once per person per calendar day** — a hangover is a
hangover. A second attempt the same day is not counted and earns a gentle shaming reply.

## Importing historical IRC data

IRC history comes from two sources, combined by `src/bin/import.rs`:

- `history/<year>.txt` — the yearly leaderboard archive from
  [krappe.fi/history](https://www.krappe.fi/history/) (`<count> <nick>` per line), committed
  to the repo since it's small and public. This is the source of truth for any year it has a
  file for.
- `#tty-krappe.log` — the irssi channel log, used only to fill in years the archive doesn't
  cover (currently the ongoing year(s) since the archive was retired). It parses `!krappe`
  lines (including the log owner's own nick-less `>` lines, attributed to `tenderi`) and
  applies the same canonicalization and once-per-day rule the live bot uses. Gitignored —
  never commit it.

Run it with:

```bash
DATABASE_URL=sqlite://krappe.db cargo run --release --bin import -- "#tty-krappe.log" history
```

Both arguments are optional and default to `#tty-krappe.log` and `history`. It's idempotent:
it **replaces** all existing IRC events with what it parsed from both sources. Telegram
events are left untouched. Stop the bot first so the database isn't being written
concurrently.

## Setup

1. Install Rust (see below) and the C build tools.
2. `cp .env.example .env` and fill in your tokens/server (see comments in that file).
3. `cargo run`

### Requirements / permissions

- **Telegram:** create a bot with [@BotFather](https://t.me/BotFather). For `/naamat` to set
  a custom admin title, add the bot to the group **as an admin** with the "Add new admins"
  right. If it lacks that right it falls back to a text-only "wasted" message. Note Telegram
  only lets a bot title admins it promoted, and titles are capped at 16 characters. The bot
  registers its command list with Telegram (`setMyCommands`) on every startup, so `/krappe`,
  `/naamat`, etc. show up in each chat's `/` command menu automatically — no manual
  BotFather step needed.
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
  bin/import.rs    one-off historical import (archive + log)
migrations/
  0001_init.sql       events + links tables
  0002_nick_aliases.sql  manual multi-nick merges (!combine / repeated /combine)
history/
  <year>.txt       krappe.fi/history archive, one file per year
```
