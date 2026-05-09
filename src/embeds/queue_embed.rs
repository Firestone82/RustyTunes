use crate::player::player::{Playlist, Track};
use crate::service::utils_service;
use serenity::all::{Color, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};

pub enum QueueEmbed<'a> {
    IsEmpty,
    Current { queue: &'a [Track], page: usize },
    TrackAdded(&'a Track),
    PlaylistAdded(&'a Playlist),
    Skipped(usize),
    TrackRemoved(&'a Track),
    InvalidIndex(usize),
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
            QueueEmbed::Current { queue, page } => {
                let mut embed: CreateEmbed = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("📜  Queue")
                    .description("Upcoming tracks:")
                    .footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())));

                let page: usize = *page.max(&1);
                let mut start: usize = (page - 1) * 10;

                if start >= queue.len() {
                    start = queue.len().saturating_sub(1);
                }

                let queue_slice: Vec<&Track> = queue.iter().skip(start).take(10).collect::<Vec<&Track>>();

                for (index, track) in queue_slice.iter().enumerate() {
                    let value = if track.added_by.is_empty() {
                        track.metadata.track_url.clone()
                    } else {
                        format!("{}\nAdded by: {}", track.metadata.track_url, track.added_by)
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
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .author(CreateEmbedAuthor::new("🎵  Track added to queue"))
                    .title(format!("**{}**", track.metadata.title))
                    .url(track.metadata.track_url.clone())
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
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("🗑️  Track removed")
                    .description(format!("**[{}]({})**", track.metadata.title, track.metadata.track_url))
            }
            QueueEmbed::InvalidIndex(index) => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Invalid index")
                    .description(format!("No track at position **{}** in the queue.", index))
            }
        }
    }
}