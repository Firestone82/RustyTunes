use crate::bot::{Context, MusicBotError};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::player::notifier::{parse_text, MessageNotify, Notifier, NotifierError};
use crate::service::embed_service::SendEmbed;
use serenity::all::{Mentionable, User};
use tokio::sync::RwLockWriteGuard;

/// Remind another user (or several) at a given time.
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

    let sender_mention = ctx.author().mention().to_string();
    let prefixed_note: Option<String> = Some(format!(
        "From {}: {}",
        sender_mention,
        note.as_deref().unwrap_or("(no message)")
    ));

    let targets: Vec<&User> = [Some(&user1), user2.as_ref(), user3.as_ref()]
        .into_iter()
        .flatten()
        .collect();

    let mut notifier: RwLockWriteGuard<Notifier> = ctx.data().notifier.write().await;
    let mut created: Vec<MessageNotify> = Vec::new();

    for target in &targets {
        let notify = notifier
            .add_message_for_user(
                guild_id,
                ctx.channel_id(),
                target.id,
                None,
                notify_at,
                prefixed_note.clone(),
            )
            .await?;
        created.push(notify);
    }
    drop(notifier);

    let mentions: String = targets
        .iter()
        .map(|u| u.mention().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    NotifyEmbed::RemindedFor { targets: &mentions, notify: &created[0] }
        .to_embed()
        .send_context(ctx, true, None)
        .await?;

    Ok(())
}
