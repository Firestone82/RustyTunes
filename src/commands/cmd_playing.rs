use crate::bot::{Context, MusicBotError};
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockReadGuard;

#[poise::command(
    prefix_command, slash_command,
)]
pub async fn playing(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let player: RwLockReadGuard<Player> = ctx.data().player.read().await;
    
    if let Some(track) = &player.current_track {
        let embed: CreateEmbed = PlayerEmbed::NowPlaying(&track).to_embed();
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
    } else {
        let embed: CreateEmbed = PlayerEmbed::NoSongPlaying.to_embed();
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
    }

    Ok(())
}
