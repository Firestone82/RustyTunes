use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use serenity::all::{Color, CreateEmbed};

/// Force the bot to leave the voice channel and clear the queue.
#[poise::command(
    prefix_command,
    slash_command,
    check = "check_author_in_voice_channel"
)]
pub async fn leave(ctx: Context<'_>) -> Result<(), MusicBotError> {
    channel_service::leave_channel(ctx).await?;
    crate::player::player::set_idle(ctx.serenity_context());

    CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("👋  Bye")
        .description("Stopped playback, cleared the queue, and left the voice channel.")
        .send_context(ctx, true, Some(15))
        .await?;

    Ok(())
}
