use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::service::embed_service;
use crate::player::player::Player;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn stop(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    match player.stop_playback().await {
        Ok(_) => {
            let embed: CreateEmbed = embed_service::create_playback_stopped_embed();
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        },
        Err(error) => {
            println!("Error stopping playback: {:?}", error);

            let embed: CreateEmbed = embed_service::create_playback_error_embed(error);
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        }
    }

    drop(player);
    Ok(())
}
