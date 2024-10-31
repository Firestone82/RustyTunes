use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    aliases("vol"),
)]
pub async fn volume(ctx: Context<'_>, volume: Option<f32>) -> Result<(), MusicBotError> {
    if let Some(mut volume) = volume {
        let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

        volume = volume.clamp(1.0, 1000.0);
        player.set_volume(volume).await?;

        PlayerEmbed::VolumeChanged(volume)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    } else {
        let player: RwLockReadGuard<Player> = ctx.data().player.read().await;
        
        PlayerEmbed::Volume(player.volume * 100.0)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    }

    Ok(())
}
