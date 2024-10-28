use crate::bot::{Context, MusicBotError};
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
)]
pub async fn playing(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
    
    if let Some(track) = &player.current_track {
        let embed: CreateEmbed = embed_service::create_now_playing_embed(track);
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
    } else {
        let embed: CreateEmbed = embed_service::create_no_song_playing_embed();
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
    }

    drop(player);
    Ok(())
}
