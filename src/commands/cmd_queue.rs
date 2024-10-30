use crate::bot::{Context, MusicBotError};
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use serenity::all::Message;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command, slash_command,
)]
pub async fn queue(ctx: Context<'_>, page: Option<usize>) -> Result<(), MusicBotError> {
    let player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    if player.queue.is_empty() {
        QueueEmbed::IsEmpty
            .to_embed()
            .send_context(ctx, true, Some(30)).await?;
    } else {
        QueueEmbed::Current {
            queue: &player.queue, 
            page: page.unwrap_or(1)
        }
            .to_embed()
            .send_context(ctx, true, Some(60)).await?;
    }

    Ok(())
}
