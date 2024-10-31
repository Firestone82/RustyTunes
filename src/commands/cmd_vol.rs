use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    aliases("vol"),
)]
pub async fn volume(ctx: Context<'_>, mut volume: Option<f32>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
    // TODO: Maybe split it into write lock and read lock?

    if let Some(vol) = volume {
        volume = vol.clamp(1.0, 1000.0).into();
    }

    if let Some(volume) = volume {
        player.set_volume(volume).await?;

        PlayerEmbed::VolumeChanged(volume)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    } else {
        PlayerEmbed::Volume(player.volume * 100.0)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    }

    Ok(())
}
