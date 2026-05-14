<img width="20%" src="assets/icon-no-bg.png" align="right" alt="Icon">
<br>

# Project for PvR: Discord MusicBot in Rust
- Author: Pavel Mikula (MIK0486)
- Took approximately 40 hours

## Project Theme
This project is a Discord bot developed in Rust, designed to play music in Discord voice channels. It uses libraries like `serenity` and `poise` for handling the Discord API, `songbird` for managing audio playback, `yt-dlp` for YouTube track streaming, and integrates with the Spotify Web API for Spotify URLs. The bot supports a full music queue, a local audio library, timed reminders, and several utility commands.

## Project Requirements
- Rust: The primary programming language for bot logic
- Serenity + Poise: Discord API wrapper and command framework for Rust
- Songbird: Voice and audio playback library for Discord
- yt-dlp: For downloading and streaming audio from YouTube
- SQLite (via sqlx): Persistent storage for guild settings and reminders
- Spotify Web API: For resolving Spotify track / playlist URLs

## Installation
### Prerequisites
- Installed rust from [rust-lang.org](https://www.rust-lang.org/tools/install)
- Installed yt-dlp from [github.com/yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp)
  - `yt-dlp` should be in the system PATH
- Installed `ffmpeg` (in the system PATH) — used by yt-dlp for post-processing **and** by the bot's loudness analyzer to measure each track's integrated LUFS for cross-track volume normalization
- YouTube Data API token from [Google Cloud Console](https://developers.google.com/youtube/registering_an_application)
- Discord bot token from [Discord Developer Portal](https://discord.com/developers/applications)
- (Optional) Spotify Client ID & Secret from [Spotify Developer Dashboard](https://developer.spotify.com/dashboard) — required for Spotify URL support
- Installed CMAKE. Required for the [audiopus_sys](https://github.com/Lakelezz/audiopus_sys) library

### Installation
1. Clone this repository
    ```bash
    git clone https://github.com/Firestone82/RustyTunes.git
    cd RustyTunes
    ```
2. Create a `.env` file in the root directory
    ```bash
    cp .env.example .env

    # Edit the .env file with your Discord bot token, YouTube API key,
    # and (optionally) Spotify client id and secret.
    ```
3. Setup database
    ```bash
    cargo install sqlx-cli
    sqlx database create
    sqlx migrate run
     ```
4. Install dependencies
    ```bash
   # yt-dlp
   sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp
   sudo chmod a+rx /usr/local/bin/yt-dlp

   # ffmpeg (required by yt-dlp and by the loudness normalizer)
   sudo apt-get install ffmpeg

   # CMAKE
   sudo apt-get install cmake
    ```
5. Build and run the bot
    ```bash
    cargo build --release
    cargo run
    ```

## Usage
All commands are available as both **prefix commands** (default prefix `!`) and **slash commands** (`/`). Use `!help` or `/help` in Discord to list every command, or `!help <command>` for details on a specific one.

## Features

### Audio Sources
- Play tracks and playlists from **YouTube** (direct URL or text search)
- Play tracks and playlists from **Spotify** (URL — resolved to YouTube for playback)
- Play files from a **local audio library** stored on the bot host
- Stream user-uploaded **Discord attachments** (audio files)
- Stream audio from **arbitrary direct URLs**

### Playback Controls
- `play <query|url>` — play a track or playlist from YouTube or Spotify (appends to queue)
- `playtop <query|url>` — same as `play` but inserts at the front of the queue
- `pause` / `resume` — pause and resume the current track
- `skip [amount]` — skip the current track (or multiple at once)
- `stop` — stop playback and clear the active track
- `playing` — show the currently playing track
- `volume [1-100]` — set the playback volume; append `!` (e.g. `volume 200!`) to opt into the extended 1–500 overdrive range
- `normalize [on|off]` — toggle session-only cross-track loudness normalization (off by default, applies to every source once enabled, resets on restart)
- `silent [on|off]` — suppress NowPlaying announcements for the session
- `join` / `leave` — manually summon or dismiss the bot from your voice channel

### Queue Management
- `queue` — paginated queue listing (10 tracks per page) with navigation buttons
- `clear` — remove every track from the queue
- `remove <index>` — remove a specific track from the queue by its 1-based index
- `shuffle` — shuffle the current queue
- `history` — show the last 10 played tracks with buttons to instantly replay any of them

### Local Audio Library
The `local` command groups subcommands for managing audio files saved on the bot host:
- `local download <url> [name]` — download an audio file from a URL into the library
- `local upload [name]` — save an uploaded Discord attachment into the library
- `local list` — list all saved local tracks
- `local play [name]` — play a saved track by name (with autocomplete and an interactive picker)
- `local rename <track> <new name>` — rename a saved track
- `local remove <track>` — delete a saved track from the library

### Reminders / Notifications
The `notify` command (alias `remind`) lets users schedule timed reminders persisted to the database:
- `notify me <when> <message>` — schedule a reminder for yourself
- `notify you <user> <when> <message>` — schedule a reminder for another user
- `notify list` — list your pending reminders
- `notify remove <id>` — cancel a pending reminder

### Utility Commands
- `wakeup <user> [count]` — drags a user briefly between voice channels to grab their attention (also available as a right-click **WakeUp!** user context menu action)
- `rename <user> [new name]` — set another member's nickname (respects role hierarchy)
- `uwu <text>` — uwuify the given text
- `uwu_me <text>` — uwuify text and post it impersonating the author via webhook
- `help [command]` — built-in help listing and per-command detail

### Quality-of-Life Behaviour
- **Auto-leave**: bot automatically leaves the voice channel when it's left alone
- **Auto-cleanup**: when the bot is kicked, dragged out, or otherwise loses its voice connection, playback state and the queue are cleaned up automatically
- **Per-guild volume persistence**: the last-set volume is remembered between sessions in SQLite
- **Cross-track loudness normalization**: opt-in via `!normalize`. When enabled, each cached track's integrated loudness is measured once (EBU R128 via ffmpeg) and a static gain is applied so songs sit at roughly the same perceived loudness without crushing in-song dynamics. Off by default, takes effect immediately on the current track, resets on restart. The target loudness (`NORMALIZE_TARGET_LUFS`, default -10) and gain clamps (`NORMALIZE_MIN_GAIN_DB`, `NORMALIZE_MAX_GAIN_DB`) are tunable via env vars — higher target = louder output
- **Per-source cache layout**: downloaded audio is filed under `cache/youtube/` or `cache/spotify/` (legacy flat-cached files still play)
- **Slash + prefix parity**: every command works both ways
- **Graceful shutdown**: handles SIGINT/SIGTERM (and Ctrl+C on Windows) to disconnect cleanly
- **Structured logging**: powered by `tracing` with environment-controlled filtering
