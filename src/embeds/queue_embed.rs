use crate::player::player::{Playlist, Track, TrackSource};
use crate::service::utils_service;
use serenity::all::{Color, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};

/// "Location" line shown under each track in queue embeds. Local files don't
/// have a useful URL; Spotify tracks without a permalink fall back to a label
/// instead of an empty cell.
fn track_location(track: &Track) -> String {
    match &track.source {
        TrackSource::Local(_) => format!("{} Local file", track.source.emoji()),
        _ if track.metadata.track_url.is_empty() => {
            format!("{} {}", track.source.emoji(), track.source.label())
        }
        _ => track.metadata.track_url.clone(),
    }
}

pub enum QueueEmbed<'a> {
    IsEmpty,
    Current { now_playing: Option<&'a Track>, queue: &'a [Track], page: usize },
    TrackAdded(&'a Track),
    PlaylistAdded(&'a Playlist),
    Skipped(usize),
    TrackRemoved(&'a Track),
    InvalidIndex(usize),
    Cleared(usize),
}

impl<'a> QueueEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            QueueEmbed::IsEmpty => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Empty queue")
                    .description("The queue is empty.")
            },
            QueueEmbed::Current { now_playing, queue, page } => {
                let mut embed: CreateEmbed = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("📜  Queue")
                    .footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())));

                if let Some(track) = now_playing {
                    let location = track_location(track);
                    let value = if track.added_by.is_empty() {
                        location
                    } else {
                        format!("{}\nAdded by: {}", location, track.added_by)
                    };
                    embed = embed.field(
                        format!("🎵  Now playing — {}", track.metadata.title),
                        value,
                        false,
                    );
                }

                if queue.is_empty() {
                    embed = embed.description("Nothing queued up.");
                    return embed;
                }

                embed = embed.description("Upcoming tracks:");

                let page: usize = *page.max(&1);
                let mut start: usize = (page - 1) * 10;

                if start >= queue.len() {
                    start = queue.len().saturating_sub(1);
                }

                let queue_slice: Vec<&Track> = queue.iter().skip(start).take(10).collect::<Vec<&Track>>();

                for (index, track) in queue_slice.iter().enumerate() {
                    let location = track_location(track);
                    let value = if track.added_by.is_empty() {
                        location
                    } else {
                        format!("{}\nAdded by: {}", location, track.added_by)
                    };
                    embed = embed.field(
                        format!("{}  {}", utils_service::number_to_emoji(index + start + 1), track.metadata.title),
                        value,
                        false,
                    );
                }

                embed
            }
            QueueEmbed::TrackAdded(track) => {
                let mut embed = CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .author(CreateEmbedAuthor::new(format!(
                        "🎵  Track added to queue  ·  {} {}",
                        track.source.emoji(), track.source.label()
                    )))
                    .title(format!("**{}**", track.metadata.title));
                if !matches!(track.source, TrackSource::Local(_))
                    && !track.metadata.track_url.is_empty()
                {
                    embed = embed.url(track.metadata.track_url.clone());
                }
                embed
            }
            QueueEmbed::PlaylistAdded(playlist) => {
                let embed: CreateEmbed = CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .author(CreateEmbedAuthor::new("🎵  Playlist added to queue"))
                    .title(format!("**{}**", playlist.title))
                    .url(playlist.playlist_url.clone())
                    .description(playlist.description.clone());

                embed.footer(CreateEmbedFooter::new(format!("Playlist length: {}", playlist.tracks.len())))
            }
            QueueEmbed::Skipped(amount) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("⏭️  Skipped")
                    .description(format!("Skipped {} track(s).", amount))
            }
            QueueEmbed::TrackRemoved(track) => {
                let body = match &track.source {
                    TrackSource::Local(_) => format!("**{}**", track.metadata.title),
                    _ if track.metadata.track_url.is_empty() => format!("**{}**", track.metadata.title),
                    _ => format!("**[{}]({})**", track.metadata.title, track.metadata.track_url),
                };
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("🗑️  Track removed")
                    .description(body)
            }
            QueueEmbed::InvalidIndex(index) => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Invalid index")
                    .description(format!("No track at position **{}** in the queue.", index))
            }
            QueueEmbed::Cleared(amount) => {
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("🧹  Queue cleared")
                    .description(format!("Removed **{}** track(s) from the queue.", amount))
            }
        }
    }
}