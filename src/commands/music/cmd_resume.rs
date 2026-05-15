use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::checks::player_checks::check_if_player_is_paused;
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    slash_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_player_is_paused"
)]
pub async fn resume(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    player.resume().await?;

    let track = player.current_track.clone();
    drop(player);

    if let Some(track) = track {
        PlayerEmbed::Resumed(&track)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    }

    Ok(())
}
