use serenity::all::{Color, CreateEmbed, CreateEmbedFooter};
use crate::player::player::Track;

pub enum PlaylistEmbed<'a> {
    IsEmpty,
    AddedToQueue { queue: &'a [Track], tracks: &'a [Track] },
}

impl<'a> PlaylistEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            PlaylistEmbed::IsEmpty => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Empty playlist")
                    .description("The playlist is empty.")
            },
            PlaylistEmbed::AddedToQueue { queue, tracks } => {
                let mut embed = CreateEmbed::new()
                    .color(Color::DARK_GREEN)
                    .title("🎵  Playlist added to queue")
                    .description("Tracks added to queue:");

                for track in *tracks {
                    embed = embed.field(track.metadata.title.clone(), track.metadata.track_url.clone(), false);
                }

                embed.footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())))
            },
        }
    }
}

// impl<'a> From<PlaylistEmbed<'a>> for CreateEmbed {
//     fn from(embed_type: PlaylistEmbed<'a>) -> Self {
//         embed_type.to_embed()
//     }
// }

impl<'a> Into<CreateEmbed> for PlaylistEmbed<'a> {
    fn into(self) -> CreateEmbed {
        self.to_embed()
    }
}