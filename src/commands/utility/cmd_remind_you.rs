use crate::bot::{Context, MusicBotError};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::player::notifier::{encode_targets, parse_text, MessageNotify, Notifier, NotifierError};
use crate::service::embed_service::SendEmbed;
use serenity::all::{Mentionable, User, UserId};
use tokio::sync::RwLockWriteGuard;

/// Remind one or more users at a given time with a single notification.
#[poise::command(prefix_command, slash_command)]
pub async fn remind_you(
    ctx: Context<'_>,
    #[description = "User to remind"] user1: User,
    #[description = "Second user (optional)"] user2: Option<User>,
    #[description = "Third user (optional)"] user3: Option<User>,
    time: String,
    #[rest]
    #[description = "Message to include in the reminder"]
    note: Option<String>,
) -> Result<(), MusicBotError> {
    let guild_id = ctx.guild_id().ok_or_else(|| {
        MusicBotError::InternalError("remind_you is only available in guilds".to_string())
    })?;

    let notify_at = match parse_text(time) {
        Ok(t) => t,
        Err(NotifierError::InvalidTimeFormat) => {
            return NotifyEmbed::InvalidNotifyFormat
                .to_embed()
                .send_context(ctx, true, None)
                .await
                .map(|_| ());
        }
        Err(other) => return Err(other.into()),
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
