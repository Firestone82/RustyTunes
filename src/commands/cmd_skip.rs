use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_player_is_playing",
    check = "check_if_queue_is_not_empty",
)]
pub async fn skip(ctx: Context<'_>, amount: Option<usize>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    let amount: usize = player.skip(amount.unwrap_or(1)).await?;
    
    let embed: CreateEmbed = embed_service::create_skip_embed(amount);
    let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;

    drop(player);
    Ok(())
}
