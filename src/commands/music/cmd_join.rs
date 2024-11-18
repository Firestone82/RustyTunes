use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::service::channel_service;

/**
* Request the bot to join current user voice channel
*/
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_voice_channel",
)]
pub async fn join(ctx: Context<'_>) -> Result<(), MusicBotError> {
    channel_service::join_user_channel(ctx).await?;
    Ok(())
}