use crate::bot::{Context, MusicBotError};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::player::notifier::{convert_time_date_from_string, convert_time_offset_from_string, parse_text, MessageNotify, Notifier};
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command, slash_command,
)]
pub async fn notify(ctx: Context<'_>, time: String, note: Option<Vec<String>>) -> Result<(), MusicBotError> {
    match parse_text(time.clone()) {
        Ok(time) => {
            let note: Option<String> = note.map(|n| n.join(" "));

            let mut notifier: RwLockWriteGuard<Notifier> = ctx.data().notifier.write().await;
            let notify: MessageNotify = notifier.add_message(ctx, time, note).await?;

            NotifyEmbed::Created(&notify)
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
        }

        Err(_) => {
            NotifyEmbed::InvalidNotifyFormat
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
        }
    }

    Ok(())
}

