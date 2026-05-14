use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::embeds::bot_embeds::BotEmbed;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::gather_service;
use serenity::all::{
    ChannelId, CreateInteractionResponse, CreateInteractionResponseMessage, GuildId,
};

/// Gather everyone in your voice channel — they tap "I'm here!" to check in.
#[poise::command(slash_command, prefix_command, check = "check_author_in_voice_channel")]
pub async fn gather(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let guild_id: GuildId = ctx.guild_id().ok_or(MusicBotError::NoGuildIdError)?;

    let voice_channel_id: ChannelId = match channel_service::get_user_voice_channel(ctx, &ctx.author().id) {
        Some(c) => c,
        None => {
            BotEmbed::CurrentUserNotInVoiceChannel
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
            return Ok(());
        }
    };

    // Acknowledge the slash command immediately — the gathering itself runs as a
    // regular channel message so it can outlive the interaction token.
    if let poise::Context::Application(app_ctx) = ctx {
        let _ = app_ctx
            .interaction
            .create_response(
                ctx.http(),
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Starting gathering…")
                        .ephemeral(true),
                ),
            )
            .await;
    }

    gather_service::start_gather(
        ctx.serenity_context(),
        guild_id,
        ctx.channel_id(),
        voice_channel_id,
        ctx.author().id,
    )
    .await?;

    Ok(())
}
