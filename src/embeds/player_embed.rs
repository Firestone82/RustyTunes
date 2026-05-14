use crate::player::player::{Track, TrackSource};
use serenity::all::{Color, CreateEmbed, CreateEmbedFooter};
use std::collections::VecDeque;
use std::path::PathBuf;

/// Description for embed bodies. Local tracks shouldn't render as a link
/// (the `file://` URL isn't useful and Discord may strip it), so plain bold
/// title is used. Spotify tracks resolved without a permalink (no API id)
/// also fall back to bold-only so we don't emit broken `[title]()` markdown.
fn track_description(track: &Track) -> String {
    match &track.source {
        TrackSource::Local(_) => format!("**{}**", track.metadata.title),
        _ if track.metadata.track_url.is_empty() => format!("**{}**", track.metadata.title),
        _ => format!("**[{}]({})**", track.metadata.title, track.metadata.track_url),
    }
}

pub enum PlayerEmbed<'a> {
    NowPlaying(&'a Track),
    NoSongPlaying,
    IsStopped,
    Stopped,
    Paused(&'a Track),
    Resumed(&'a Track),
    Volume(f32),
    VolumeChanged(f32),
    SilentState(bool),
    NormalizeState(bool),
    Skipped(usize),
    Shuffled,
    Search(&'a [Track]),
    SearchExpired,
    SearchCancelled,
    NoResults(String),
    QuotaExceeded,
    PlaybackErrorEmbed(String),
    InactivityLeave,
    History(&'a VecDeque<Track>),
    HistoryEmpty,
    Downloading(&'a str),
    Downloaded(&'a str),
    DownloadFailed(String),
    LocalFiles(&'a [PathBuf]),
    LocalEmpty,
    LocalNoMatch(&'a str),
    LocalRemoved(&'a str),
    LocalRenamed { old: &'a str, new: &'a str },
    LocalAmbiguous(&'a [PathBuf]),
    LocalPickToPlay(&'a [PathBuf]),
    LocalPickToRemove(&'a [PathBuf]),
}

impl<'a> PlayerEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            PlayerEmbed::NowPlaying(track) => {
                let song = track_description(track);
                let author = if track.metadata.channel.is_empty() {
                    "—".to_string()
                } else {
                    track.metadata.channel.clone()
                };
                let source = format!("{} {}", track.source.emoji(), track.source.label());

                let mut embed = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🎵  Now playing:")
                    .field("Song", song, false)
                    .field("Author", author, true)
                    .field("Source", source, true);
                if !track.added_by.is_empty() {
                    embed = embed.footer(CreateEmbedFooter::new(format!("Added by {}", track.added_by)));
                }
                embed
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
            PlayerEmbed::Paused(track) => {
                CreateEmbed::new()
                    .color(Color::ORANGE)
                    .title("⏸️  Paused")
                    .description(track_description(track))
            },
            PlayerEmbed::Resumed(track) => {
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("▶️  Resumed")
                    .description(track_description(track))
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
            PlayerEmbed::NormalizeState(on) => {
                let (title, body) = if *on {
                    (
                        "🎚️  Normalization on",
                        "Cross-track loudness normalization is **on** — every upcoming track gets its measured gain applied so songs sit at roughly the same perceived loudness.",
                    )
                } else {
                    (
                        "🎚️  Normalization off",
                        "Cross-track loudness normalization is **off** — upcoming tracks play at their original loudness.",
                    )
                };
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title(title)
                    .description(body)
            },
            PlayerEmbed::SilentState(on) => {
                let (title, body) = if *on {
                    (
                        "🔕  Silent mode on",
                        "Now Playing announcements are **suppressed** for this session.",
                    )
                } else {
                    (
                        "🔔  Silent mode off",
                        "Now Playing announcements are back on for this session.",
                    )
                };
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title(title)
                    .description(body)
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
            PlayerEmbed::SearchCancelled => {
                CreateEmbed::new()
                    .color(Color::DARK_GREY)
                    .title("✖  Search cancelled")
                    .description("No track was added to the queue.")
            }
            PlayerEmbed::NoResults(query) => {
                CreateEmbed::new()
                    .color(Color::DARK_GOLD)
                    .title("🔎  No results")
                    .description(format!("No tracks found for: **{}**", query))
            }
            PlayerEmbed::QuotaExceeded => {
                CreateEmbed::new()
                    .color(Color::DARK_GOLD)
                    .title("🚧  YouTube API quota exceeded")
                    .description("The bot has hit YouTube's daily search quota. Please try again later or ask the owner to provide a fresh API key.")
            }
            PlayerEmbed::PlaybackErrorEmbed(message) => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Playback error")
                    .description(message.clone())
            }
            PlayerEmbed::InactivityLeave => {
                CreateEmbed::new()
                    .color(Color::DARK_GOLD)
                    .title("👋  Leaving voice channel")
                    .description("No tracks have been queued for 5 minutes — leaving the voice channel.")
            }
            PlayerEmbed::History(history) => {
                let mut embed = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("📜  Recently played")
                    .description("Pick a number to replay a track:");

                for (i, track) in history.iter().rev().enumerate() {
                    let location = match &track.source {
                        TrackSource::Local(_) => format!("{} Local file", track.source.emoji()),
                        _ if track.metadata.track_url.is_empty() => {
                            format!("{} {}", track.source.emoji(), track.source.label())
                        }
                        _ => track.metadata.track_url.clone(),
                    };
                    embed = embed.field(
                        format!("{}. {}", i + 1, track.metadata.title),
                        location,
                        false,
                    );
                }
                embed
            }
            PlayerEmbed::HistoryEmpty => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("📜  No history")
                    .description("No tracks have been played yet.")
            }
            PlayerEmbed::Downloading(url) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("⬇️  Downloading")
                    .description(format!("Fetching `{}`…", url))
            }
            PlayerEmbed::Downloaded(name) => {
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("✅  Downloaded")
                    .description(format!("Saved **{}** to local library.", name))
            }
            PlayerEmbed::DownloadFailed(reason) => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Download failed")
                    .description(reason.clone())
            }
            PlayerEmbed::LocalFiles(files) => {
                local_listing_embed(
                    "📁  Local library",
                    "Saved tracks (use `!local play <name>`):",
                    files,
                )
            }
            PlayerEmbed::LocalEmpty => {
                CreateEmbed::new()
                    .color(Color::DARK_GOLD)
                    .title("📁  Local library empty")
                    .description("No downloaded tracks yet. Use `!local download <url>` to add one.")
            }
            PlayerEmbed::LocalNoMatch(query) => {
                CreateEmbed::new()
                    .color(Color::DARK_GOLD)
                    .title("🔎  No local match")
                    .description(format!("No saved track matches **{}**.", query))
            }
            PlayerEmbed::LocalRemoved(name) => {
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("🗑️  Removed")
                    .description(format!("Deleted **{}** from local library.", name))
            }
            PlayerEmbed::LocalRenamed { old, new } => {
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("✏️  Renamed")
                    .description(format!("**{}** → **{}**", old, new))
            }
            PlayerEmbed::LocalAmbiguous(files) => {
                local_listing_embed(
                    "🔎  Multiple matches",
                    "Be more specific — these all matched:",
                    files,
                )
            }
            PlayerEmbed::LocalPickToPlay(files) => {
                local_listing_embed(
                    "📁  Pick a track to play",
                    "Multiple matches — choose one:",
                    files,
                )
            }
            PlayerEmbed::LocalPickToRemove(files) => {
                local_listing_embed(
                    "🗑️  Pick a track to remove",
                    "Multiple matches — choose one to delete:",
                    files,
                )
            }
        }
    }
}

fn local_listing_embed(title: &str, description: &str, files: &[PathBuf]) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title(title.to_string())
        .description(description.to_string());

    for (i, path) in files.iter().enumerate() {
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?");
        embed = embed.field(format!("{}. {}", i + 1, name), "\u{200b}", false);
    }
    embed
}