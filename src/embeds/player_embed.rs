use crate::player::player::Track;
use serenity::all::{Color, CreateEmbed};

pub enum PlayerEmbed<'a> {
    NowPlaying(&'a Track),
    IsStopped,
    Volume { volume: f32 },
    VolumeChanged { volume: f32 },
    NoSongPlaying,
    Skipped { amount: usize },
}

impl<'a> PlayerEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            PlayerEmbed::NowPlaying(track) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🎵  Now playing")
                    .description(format!("**[{}]({})**", track.metadata.title, track.metadata.track_url))
            },
            PlayerEmbed::IsStopped => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("⏹️  Playback stopped")
                    .description("The playback has been stopped.")
            },
            PlayerEmbed::Volume { volume } => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔊  Volume")
                    .description(format!("Volume is set to {}%.", volume))
            },
            PlayerEmbed::VolumeChanged { volume } => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔊  Volume changed")
                    .description(format!("Volume set to {}%.", volume))
            },
            PlayerEmbed::NoSongPlaying => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  No song playing")
                    .description("No song is currently playing.")
            },
            PlayerEmbed::Skipped { amount } => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("⏭️  Skipped")
                    .description(format!("Skipped {} track(s).", amount))
            },
        }
    }
}