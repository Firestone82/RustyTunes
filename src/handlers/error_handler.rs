use crate::bot::{MusicBotData, MusicBotError};
use crate::embeds::bot::bot_embeds::BotEmbed;
use async_trait::async_trait;
use songbird::{Event, EventContext, EventHandler};

/// Delete the user's prefix-command invocation after 30s so the channel
/// doesn't get cluttered. Slash commands clean themselves up.
pub fn schedule_prefix_delete(ctx: poise::Context<'_, MusicBotData, MusicBotError>) {
    if let poise::Context::Prefix(prefix_ctx) = ctx {
        let http = ctx.serenity_context().http.clone();
        let channel_id = prefix_ctx.msg.channel_id;
        let message_id = prefix_ctx.msg.id;
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            let _ = http.delete_message(channel_id, message_id, None).await;
        });
    }
}

/// Songbird event handler for `TrackEvent::Error` — logs and continues.
pub struct ErrorHandler;

#[async_trait]
impl EventHandler for ErrorHandler {
    async fn act(
        &self,
        _e: &EventContext<'_>,
    ) -> Option<Event> {
        tracing::error!("Track error event: {:?}", _e);
        None
    }
}

pub async fn handle(error: poise::FrameworkError<'_, MusicBotData, MusicBotError>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            panic!("Failed to start bot: {:?}", error)
        }

        // `error` already carries the "Whoops…" prefix in its Display impl,
        // so render it raw — wrapping it again would nest the prefix.
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::error!("Error in command `{}`: {:?}", ctx.command().name, error);
            let embed = BotEmbed::Error(error).to_embed();
            let _ = ctx
                .send(poise::CreateReply::default().embed(embed).reply(true))
                .await;
            schedule_prefix_delete(ctx);
        }

        poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
            if let Some(error) = error {
                let _ = ctx.reply(error.to_string()).await;
            }
            schedule_prefix_delete(ctx);
        }

        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("Error while handling error: {}", e);
            }
        }
    }
}
