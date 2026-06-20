# RustyTunes

> **VŠB-TUO** — School project · Programming in Rust (PvR)

![Rust](https://img.shields.io/badge/Rust-2021-orange) ![Discord](https://img.shields.io/badge/Discord-Bot-5865F2)

## About

A feature-rich Discord music bot written in Rust, developed as a school project for the PvR (Programming in Rust) course at VŠB-TUO. Supports YouTube, Spotify, local audio files, and direct URLs. Includes a persistent queue, per-guild volume memory, loudness normalization, timed reminders, and various utility commands.

## Features

**Audio sources:** YouTube URL or search, Spotify URL resolution, local audio library, Discord attachments, direct URLs

**Playback:** play, pause, resume, skip, stop, volume control, per-track loudness normalization

**Queue:** paginated display, shuffle, clear, remove individual tracks, playback history

**Local library:** upload and download audio files, replay saved tracks by name

**Reminders:** schedule persistent notifications for yourself or others

**Utilities:** wake-up pings, nickname changer, text transformations, `/help` command

## Requirements

- Rust stable toolchain — install via [rustup.rs](https://rustup.rs/)
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) in system PATH
- [`ffmpeg`](https://ffmpeg.org/) in system PATH
- CMake (required by `audiopus_sys`)
- Discord bot token — create at [discord.com/developers](https://discord.com/developers/applications)
- YouTube Data API v3 key — from [Google Cloud Console](https://console.cloud.google.com/)
- *(Optional)* Spotify API credentials for Spotify URL support

## Setup

1. Install system dependencies:
   ```bash
   # Debian/Ubuntu
   apt-get install ffmpeg cmake

   # Install yt-dlp
   pip install yt-dlp
   ```

2. Clone the repository:
   ```bash
   git clone https://github.com/Firestone82/RustyTunes.git
   cd RustyTunes
   ```

3. Copy `.env.example` to `.env` and fill in your Discord token, YouTube API key, and optional Spotify credentials.

4. Set up the database:
   ```bash
   sqlx database create
   sqlx migrate run
   ```

5. Build and run:
   ```bash
   cargo build --release
   cargo run --release
   ```

### Discord bot setup

1. Go to [discord.com/developers/applications](https://discord.com/developers/applications) and create a new application.
2. Under **Bot**, enable the **Message Content Intent** and **Server Members Intent**.
3. Copy the bot token into `.env`.
4. Invite the bot using the OAuth2 URL generator with `bot` + `applications.commands` scopes and `Connect`, `Speak`, and `Send Messages` permissions.

## License

This project was created as a school assignment at VŠB-TUO.
