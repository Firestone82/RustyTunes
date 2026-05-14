use crate::bot::{MusicBotData, MusicBotError};
use crate::embeds::bot_embeds::BotEmbed;
use async_trait::async_trait;
use songbird::{Event, EventContext, EventHandler};

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

pub struct ErrorHandler;

#[async_trait]
impl EventHandler for ErrorHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        tracing::error!("Track error event: {:?}", _e);
        None
    }
}

pub async fn handle(error: poise::FrameworkError<'_, MusicBotData, MusicBotError>) {
    match error {
        // Bot failed to start
        poise::FrameworkError::Setup { error, .. } => {
            panic!("Failed to start bot: {:?}", error)
        }

        // Command failed to execute. `error` is already a MusicBotError whose
        // Display impl carries the user-facing prefix — wrapping it again would
        // produce nested "Whoops, an internal error occurred:" prefixes.
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::error!("Error in command `{}`: {:?}", ctx.command().name, error);
            let embed = BotEmbed::Error(error).to_embed();
            let _ = ctx
                .send(poise::CreateReply::default().embed(embed).reply(true))
                .await;
            schedule_prefix_delete(ctx);
        }

        // Command check failed
        poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
            if let Some(error) = error {
                let _ = ctx.reply(error.to_string()).await;
            }
            schedule_prefix_delete(ctx);
        }

        // Unmatched errors
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("Error while handling error: {}", e);
            }
        }
    }
}
