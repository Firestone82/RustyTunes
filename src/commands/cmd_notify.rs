use crate::bot::{Context, MusicBotError};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::player::notifier::{convert_time_string, MessageNotify, Notifier};
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
)]
pub async fn notify_me(ctx: Context<'_>, time: Vec<String>) -> Result<(), MusicBotError> {
    let mut notifier: RwLockWriteGuard<Notifier> = ctx.data().notifier.write().await;
    
    let notify: MessageNotify = notifier.add_message(ctx, convert_time_string(&time.join(" "))?).await?;

    NotifyEmbed::Created(&notify)
        .to_embed()
        .send_context(ctx, true, None)
        .await?;
    
    Ok(())
}

