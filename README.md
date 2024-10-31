<img width="20%" src="assets/icon-no-bg.png" align="right" alt="Icon">
<br>

# Project for PvR (Discord MusicBot in Rust)
- Author: Pavel Mikula (MIK0486)
- Took approximately 40 hours

## Project Theme
This project is a simple Discord bot developed in Rust, designed to play music in Discord voice channels. It uses libraries like `serenity` for handling the Discord API and `songbird` for managing audio playback and `youtube-api` for track download. The bot allows users to add, play, pause, and skip songs in a queue directly within a Discord server.

## Project Requirements
- Rust: The primary programming language for bot logic
- Serenity: Discord API wrapper for Rust
- Songbird: Voice and audio playback library for Discord
- Youtube-dl: For downloading and streaming audio from YouTube

## Instalation
### Prerequisites
- Installed rust from [rust-lang.org](https://www.rust-lang.org/tools/install)
- Installed youtube-dl from [ytdl-org.github.io](https://ytdl-org.github.io/youtube-dl/)
  - `ffmpeg` is required for youtube-dl to work properly
  - `youtube-dl` should be in the system PATH
- Discord bot token from [Discord Developer Portal](https://discord.com/developers/applications)

### Installation
1. Clone this repository
    ```bash
    git clone https://github.com/Firestone82/RustyTunes.git
    cd RustyTunes
    ```
2. Create a `.env` file in the root directory
    ```bash
    cp .env.example .env
    
    # Edit the .env file and add your Discord bot token
    ```
3. Setup database
    ```bash
    cargo install sqlx-cli
    sqlx database create
    sqlx migrate run
     ```
4. Install dependencies
    ```bash
    cargo build
    ```
5. Build and run the bot
    ```bash
    cargo build --release
    cargo run
    ```

## Features
- Play music from YouTube and other audio sources in voice channels
- Queue management: add, remove, or reorder songs
- Basic music controls: play, pause, resume, and skip tracks
- Volume control