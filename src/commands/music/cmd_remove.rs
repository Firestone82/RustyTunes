use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::checks::player_checks::check_if_queue_is_not_empty;
use crate::embeds::music::queue_embed::QueueEmbed;
use crate::player::player::Player;
use crate::player::track::PlaybackError;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

/// Remove a track from the queue by 1-based index.
#[poise::command(prefix_command, slash_command, check = "check_author_in_same_voice_channel", check = "check_if_queue_is_not_empty")]
pub async fn remove(ctx: Context<'_>, index: usize) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    match player.remove_from_queue(index).await {
        Ok(track) => {
            drop(player);
            QueueEmbed::TrackRemoved(&track)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
        }
        Err(PlaybackError::InvalidQueueIndex(i)) => {
            drop(player);
            QueueEmbed::InvalidIndex(i)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}
