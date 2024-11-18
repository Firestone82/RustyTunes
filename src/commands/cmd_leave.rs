use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::service::channel_service;

/**
* Force the bot to leave the voice channel
*/
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_voice_channel",
)]
pub async fn leave(ctx: Context<'_>) -> Result<(), MusicBotError> {
    channel_service::leave_channel(ctx).await?;
    Ok(())
}
