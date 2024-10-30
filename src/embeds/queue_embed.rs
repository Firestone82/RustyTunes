use crate::player::player::Track;
use crate::service::utils_service;
use serenity::all::{Color, CreateEmbed, CreateEmbedFooter};

pub enum QueueEmbed<'a> {
    IsEmpty,
    Current { queue: &'a [Track], page: usize },
    TrackAdded { queue: &'a [Track], track: &'a Track },
    PlaylistAdded { queue: &'a [Track], tracks: &'a [Track] },
    Skipped(usize),
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
                    embed = embed.field(
                        format!("{}  {}", utils_service::number_to_emoji(index + start + 1), track.metadata.title),
                        &track.metadata.track_url,
                        false,
                    );
                }

                embed
            }
            QueueEmbed::TrackAdded { queue, track } => {
                CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("🎵  Track added to queue")
                    .description(format!("**[{}]({})**", track.metadata.title, track.metadata.track_url))
                    .footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())))
            }
            QueueEmbed::PlaylistAdded { queue, tracks } => {
                let mut embed: CreateEmbed = CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("🎵  Playlist added to queue")
                    .description("Tracks added to queue:");

                for track in *tracks {
                    embed = embed.field(track.metadata.title.clone(), track.metadata.track_url.clone(), false);
                }

                embed.footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())))
            }
            QueueEmbed::Skipped(amount) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("⏭️  Skipped")
                    .description(format!("Skipped {} track(s).", amount))
            }
        }
    }
}