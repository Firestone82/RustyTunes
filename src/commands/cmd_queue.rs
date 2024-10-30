use crate::bot::{Context, MusicBotError};
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command, slash_command,
)]
pub async fn queue(ctx: Context<'_>, page: Option<usize>) -> Result<(), MusicBotError> {
    let player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    if player.queue.is_empty() {
        let embed: CreateEmbed = embed_service::create_empty_queue_embed();
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
    } else {
        let embed: CreateEmbed = embed_service::create_queue_embed(&player.queue, page.unwrap_or(1));
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
    }

    Ok(())
}
