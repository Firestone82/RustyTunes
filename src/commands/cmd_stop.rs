use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::{
    check_author_in_same_voice_channel,
    check_if_player_is_playing
};
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_player_is_playing",
)]
pub async fn stop(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    player.stop_playback().await?;
    
    let embed: CreateEmbed = embed_service::create_playback_stopped_embed();
    let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;

    Ok(())
}
