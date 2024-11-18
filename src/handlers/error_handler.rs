use crate::bot::{MusicBotData, MusicBotError};
use async_trait::async_trait;
use songbird::{Event, EventContext, EventHandler};

pub struct ErrorHandler;

#[async_trait]
impl EventHandler for ErrorHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        println!("Error detected. Error handler called to action. {:?}", _e);
        None
    }
}

pub async fn handle(error: poise::FrameworkError<'_, MusicBotData, MusicBotError>) {
    match error {
        // Bot failed to start
        poise::FrameworkError::Setup { error, .. } => {
            panic!("Failed to start bot: {:?}", error)
        },

        // Command failed to execute
        poise::FrameworkError::Command { error, ctx, .. } => {
            println!("Error in command `{}`: {:?}", ctx.command().name, error,);
            let _ = ctx.reply(error.to_string()).await;
        }

        // Command check failed
        poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
            if let Some(error) = error {
                let _ = ctx.reply(error.to_string()).await;
            }
        }

        // Unmatched errors
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Error while handling error: {}", e)
            }
        }

    }
}