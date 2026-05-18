use crate::bot::{MusicBotData, MusicBotError};
use serenity::all::FullEvent;
use serenity::prelude::Context as SerenityContext;

/// Handle `Message` events by scanning the message text for keyword
/// matches and reacting with the configured emoji for each match.
pub async fn handle(
    ctx: &SerenityContext,
    event: &FullEvent,
    data: &MusicBotData,
) -> Result<(), MusicBotError> {
    let FullEvent::Message { new_message } = event else {
        return Ok(());
    };

    if new_message.author.bot {
        return Ok(());
    }

    let reactions = data.emoticon_service.detect_reactions(&new_message.content);
    if reactions.is_empty() {
        return Ok(());
    }

    for reaction in reactions {
        if let Err(e) = new_message.react(&ctx.http, reaction).await {
            tracing::warn!(
                "Failed to add emoticon reaction to message {}: {:?}",
                new_message.id,
                e
            );
        }
    }

    Ok(())
}
