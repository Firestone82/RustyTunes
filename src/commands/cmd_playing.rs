use crate::bot::{Context, MusicBotError};
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockReadGuard;

#[poise::command(
    prefix_command, slash_command,
)]
pub async fn playing(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let player: RwLockReadGuard<Player> = ctx.data().player.read().await;
    
    if let Some(track) = &player.current_track {
        PlayerEmbed::NowPlaying(&track)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    } else {
        PlayerEmbed::NoSongPlaying
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    }

    Ok(())
}
