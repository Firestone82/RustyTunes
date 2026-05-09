use crate::player::player::Track;
use serenity::all::{Color, CreateEmbed};

pub enum PlayerEmbed<'a> {
    NowPlaying(&'a Track),
    NoSongPlaying,
    IsStopped,
    Stopped,
    Volume(f32),
    VolumeChanged(f32),
    Skipped(usize),
    Shuffled,
    Search(&'a [Track]),
    SearchExpired
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
            PlayerEmbed::NoSongPlaying => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  No song playing")
                    .description("No song is currently playing.")
            },
            PlayerEmbed::IsStopped => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("⏹️  Playback stopped")
                    .description("The playback has been stopped.")
            },
            PlayerEmbed::Stopped => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("⏹️  Playback stopped")
                    .description("The playback has been stopped.")
            },
            PlayerEmbed::Volume(volume) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔊  Volume")
                    .description(format!("Volume is set to {}%.", volume))
            },
            PlayerEmbed::VolumeChanged(volume) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔊  Volume changed")
                    .description(format!("Volume set to {}%.", volume))
            },
            PlayerEmbed::Skipped(amount) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("⏭️  Skipped")
                    .description(format!("Skipped {} track(s).", amount))
            },
            PlayerEmbed::Shuffled => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔀  Shuffle")
                    .description("Queue has been shuffled.")
            },
            PlayerEmbed::Search(tracks) => {
                let mut embed: CreateEmbed = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔍  Search results")
                    .description("Choose a track to add to the queue:");

                for (index, track) in tracks.iter().enumerate() {
                    embed = embed.field(format!("{}.  {}", index + 1, track.metadata.title), track.metadata.track_url.clone(), false);
                }

                embed
            },
            PlayerEmbed::SearchExpired => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Search expired")
                    .description("The search has expired. Please try again.")
            }
        }
    }
}