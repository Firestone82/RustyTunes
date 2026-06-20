<img width="18%" src="assets/icon-no-bg.png" align="right" alt="Icon">

# RustyTunes

> **VŠB-TUO** — School project · Programming in Rust (PvR)

![Rust](https://img.shields.io/badge/Rust-2021-orange) ![Discord](https://img.shields.io/badge/Discord-Bot-5865F2)

A feature-rich Discord music bot written in Rust. Supports YouTube, Spotify, local audio files, and direct URLs. Includes a persistent queue, per-guild volume memory, loudness normalization, timed reminders, and various utility commands.

## Features

### Audio Sources
- YouTube (direct URL or text search)
- Spotify (URL — resolved to YouTube for playback)
- Local audio library stored on the bot host
- Discord attachment uploads (audio files)
- Arbitrary direct URLs

### Playback Controls
| Command | Description |
|---------|-------------|
| `play <query\|url>` | Play a track or playlist, append to queue |
| `playtop <query\|url>` | Same, but insert at front of queue |
| `pause` / `resume` | Pause and resume the current track |
| `skip [amount]` | Skip current track (or N tracks) |
| `stop` | Stop playback and clear the active track |
| `playing` | Show the currently playing track |
| `volume [1-100]` | Set volume; append `!` for overdrive (1–500) |
| `normalize [on\|off]` | Toggle cross-track loudness normalization (EBU R128) |
| `silent [on\|off]` | Suppress Now Playing announcements |
| `join` / `leave` | Summon or dismiss from voice channel |

### Queue Management
| Command | Description |
|---------|-------------|
| `queue` | Paginated queue (10 tracks/page) with navigation |
| `clear` | Remove all tracks from the queue |
| `remove <index>` | Remove a specific track by 1-based index |
| `shuffle` | Shuffle the current queue |
| `history` | Last 10 played tracks with replay buttons |

### Local Audio Library
| Command | Description |
|---------|-------------|
| `local download <url> [name]` | Download audio from URL into library |
| `local upload [name]` | Save a Discord attachment into library |
| `local list` | List all saved tracks |
| `local play [name]` | Play a saved track (with autocomplete) |
| `local rename <track> <name>` | Rename a saved track |
| `local remove <track>` | Delete a saved track |

### Reminders
| Command | Description |
|---------|-------------|
| `notify me <when> <msg>` | Schedule a reminder for yourself |
| `notify you <user> <when> <msg>` | Schedule a reminder for another user |
| `notify list` | List your pending reminders |
| `notify remove <id>` | Cancel a pending reminder |

### Utilities
| Command | Description |
|---------|-------------|
| `wakeup <user> [count]` | Drag a user between voice channels to get attention |
| `rename <user> [name]` | Set a member's nickname |
| `uwu <text>` | Uwuify text |
| `help [command]` | Show command list or per-command help |

All commands are available as both prefix commands (default `!`) and slash commands (`/`).

### Quality-of-Life
- Auto-leave when alone in channel
- Per-guild volume persistence (SQLite)
- Cross-track loudness normalization (opt-in, EBU R128 via ffmpeg)
- Slash + prefix parity
- Graceful SIGINT/SIGTERM shutdown
- Structured logging via `tracing`

## Requirements

- Rust stable toolchain — [rustup.rs](https://rustup.rs/)
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) in system PATH
- [`ffmpeg`](https://ffmpeg.org/) in system PATH
- CMake (required by `audiopus_sys`)
- Discord bot token — [discord.com/developers](https://discord.com/developers/applications)
- YouTube Data API v3 key — [Google Cloud Console](https://console.cloud.google.com/)
- *(Optional)* Spotify Client ID & Secret — [Spotify Developer Dashboard](https://developer.spotify.com/dashboard)

## Setup

1. Install system dependencies:
   ```bash
   # Debian/Ubuntu
   sudo apt-get install ffmpeg cmake
   sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp \
     -o /usr/local/bin/yt-dlp && sudo chmod a+rx /usr/local/bin/yt-dlp
   ```

2. Clone the repository:
   ```bash
   git clone https://github.com/Firestone82/RustyTunes.git
   cd RustyTunes
   ```

3. Copy `.env.example` to `.env` and fill in your Discord token, YouTube API key, and optional Spotify credentials.

4. Set up the database:
   ```bash
   cargo install sqlx-cli
   sqlx database create
   sqlx migrate run
   ```

5. Build and run:
   ```bash
   cargo build --release
   cargo run --release
   ```

### Discord bot setup

1. Create an application at [discord.com/developers/applications](https://discord.com/developers/applications).
2. Under **Bot**, enable **Message Content Intent** and **Server Members Intent**.
3. Copy the bot token into `.env` as `DISCORD_TOKEN`.
4. Invite the bot via the OAuth2 URL generator with `bot` + `applications.commands` scopes and `Connect`, `Speak`, and `Send Messages` permissions.

## License

This project was created as a school assignment at VŠB-TUO.
