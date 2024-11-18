use tokio::sync::RwLockReadGuard;
use crate::bot::{Context, MusicBotError};
use crate::embeds::player_embed::PlayerEmbed;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;

pub async fn check_if_player_is_playing(ctx: Context<'_>) -> Result<bool, MusicBotError> {
    let player: RwLockReadGuard<Player> = ctx.data().player.read().await;

    if player.is_playing {
        Ok(true)
    } else {
         PlayerEmbed::NoSongPlaying
             .to_embed()
             .send_context(ctx, true, Some(30))
             .await?;

        Ok(false)
    }
}

pub async fn check_if_queue_is_not_empty(ctx: Context<'_>) -> Result<bool, MusicBotError> {
    let player: RwLockReadGuard<Player> = ctx.data().player.read().await;

    if player.queue.is_empty() {
        QueueEmbed::IsEmpty
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;

        Ok(false)
    } else {
        Ok(true)
    }
}