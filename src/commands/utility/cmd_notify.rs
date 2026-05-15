use crate::bot::{Context, MusicBotError};
use crate::embeds::utility::notify_embeds::NotifyEmbed;
use crate::service::embed_service::SendEmbed;
use crate::service::notifier_service::{encode_targets, MessageNotify, Notifier, NotifierError};
use crate::utils::time_utils::{parse_text, TimeParseError};
use serenity::all::{Mentionable, User, UserId};
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

/// Manage timed notifications: me, you, list, remove.
#[poise::command(prefix_command, slash_command, subcommands("me", "you", "list", "remove"), subcommand_required, aliases("remind"))]
pub async fn notify(_ctx: Context<'_>) -> Result<(), MusicBotError> {
    Ok(())
}

/// Slash-command alias of `/notify` — same `me`/`you`/`list`/`remove` subcommands.
#[poise::command(prefix_command, slash_command, subcommands("me", "you", "list", "remove"), subcommand_required)]
pub async fn remind(_ctx: Context<'_>) -> Result<(), MusicBotError> {
    Ok(())
}

/// Schedule a notification for yourself: `/notify me 10s drink water`.
#[poise::command(prefix_command, slash_command)]
pub async fn me(
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
        Err(TimeParseError::InvalidTimeFormat) => {
            NotifyEmbed::InvalidNotifyFormat
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
        }
    }

    Ok(())
}

/// Remind one or more users at a given time: `/notify you @user 10s drink water`.
#[poise::command(prefix_command, slash_command)]
pub async fn you(
    ctx: Context<'_>,
    #[description = "User to remind"] user1: User,
    #[description = "Second user (optional)"] user2: Option<User>,
    #[description = "Third user (optional)"] user3: Option<User>,
    time: String,
    #[rest]
    #[description = "Message to include in the reminder"]
    note: Option<String>,
) -> Result<(), MusicBotError> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| MusicBotError::InternalError("notify you is only available in guilds".to_string()))?;

    let notify_at = match parse_text(time) {
        Ok(t) => t,
        Err(TimeParseError::InvalidTimeFormat) => {
            return NotifyEmbed::InvalidNotifyFormat
                .to_embed()
                .send_context(ctx, true, None)
                .await
                .map(|_| ());
        }
    };

    let targets: Vec<&User> = [Some(&user1), user2.as_ref(), user3.as_ref()]
        .into_iter()
        .flatten()
        .collect();

    let target_ids: Vec<UserId> = targets.iter().map(|u| u.id).collect();

    let user_note = note.unwrap_or_default();
    let stored_note = encode_targets(&target_ids, &user_note);

    let mut notifier: RwLockWriteGuard<Notifier> = ctx.data().notifier.write().await;
    let created: MessageNotify = notifier
        .add_message_for_user(
            guild_id,
            ctx.channel_id(),
            ctx.author().id,
            None,
            notify_at,
            Some(stored_note),
        )
        .await?;
    drop(notifier);

    let mentions: String = targets
        .iter()
        .map(|u| u.mention().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let display = created.display_note();
    NotifyEmbed::RemindedFor {
        targets: &mentions,
        notify: &created,
        note: display.as_deref(),
    }
    .to_embed()
    .send_context(ctx, true, None)
    .await?;

    Ok(())
}

/// List your pending notifications in this guild.
#[poise::command(prefix_command, slash_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| MusicBotError::InternalError("Notify is only available in guilds".to_string()))?;

    let notifier: RwLockReadGuard<Notifier> = ctx.data().notifier.read().await;
    let items = notifier.list_for_user(ctx.author().id, guild_id);

    NotifyEmbed::List(&items)
        .to_embed()
        .send_context(ctx, true, None)
        .await?;

    Ok(())
}

/// Remove one of your notifications by id.
#[poise::command(prefix_command, slash_command)]
pub async fn remove(
    ctx: Context<'_>,
    id: i64,
) -> Result<(), MusicBotError> {
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| MusicBotError::InternalError("Notify is only available in guilds".to_string()))?;

    let mut notifier: RwLockWriteGuard<Notifier> = ctx.data().notifier.write().await;
    match notifier
        .remove_for_user(ctx.author().id, guild_id, id)
        .await
    {
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
