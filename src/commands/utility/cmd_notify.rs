use crate::bot::{Context, MusicBotError};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::player::notifier::{parse_text, MessageNotify, Notifier, NotifierError};
use crate::service::embed_service::SendEmbed;
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

/// Schedule a notification: `/notify 10s remember to drink water`.
#[poise::command(prefix_command, slash_command)]
pub async fn notify(
    ctx: Context<'_>,
    time: String,
    #[rest] note: Option<String>,
) -> Result<(), MusicBotError> {
    match parse_text(time) {
        Ok(notify_at) => {
            let mut notifier: RwLockWriteGuard<Notifier> = ctx.data().notifier.write().await;
            let notify: MessageNotify = notifier.add_message(ctx, notify_at, note).await?;

            NotifyEmbed::Created(&notify)
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
        }
        Err(NotifierError::InvalidTimeFormat) => {
            NotifyEmbed::InvalidNotifyFormat
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
        }
        Err(other) => return Err(other.into()),
    }

    Ok(())
}

/// List your pending notifications in this guild.
#[poise::command(prefix_command, slash_command, rename = "notify_list")]
pub async fn notify_list(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        MusicBotError::InternalError("Notify is only available in guilds".to_string())
    })?;

    let notifier: RwLockReadGuard<Notifier> = ctx.data().notifier.read().await;
    let items = notifier.list_for_user(ctx.author().id, guild_id);

    NotifyEmbed::List(&items)
        .to_embed()
        .send_context(ctx, true, None)
        .await?;

    Ok(())
}

/// Remove one of your notifications by id.
#[poise::command(prefix_command, slash_command, rename = "notify_remove")]
pub async fn notify_remove(ctx: Context<'_>, id: i64) -> Result<(), MusicBotError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        MusicBotError::InternalError("Notify is only available in guilds".to_string())
    })?;

    let mut notifier: RwLockWriteGuard<Notifier> = ctx.data().notifier.write().await;
    match notifier.remove_for_user(ctx.author().id, guild_id, id).await {
        Ok(removed) => {
            NotifyEmbed::Removed(&removed)
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
        }
        Err(NotifierError::NotFound) => {
            NotifyEmbed::NotFound
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
        }
        Err(other) => return Err(other.into()),
    }

    Ok(())
}
