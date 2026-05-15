use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::embeds::bot_embeds::BotEmbed;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::gather_service::{self, GatherState};
use serenity::all::{
    ChannelId, Color, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage,
    GuildId, Mentionable, User,
};
use std::sync::Arc;

/// Gathering commands — gather everyone in your voice channel.
#[poise::command(
    slash_command,
    prefix_command,
    subcommands("start", "expect"),
    subcommand_required
)]
pub async fn gather(_ctx: Context<'_>) -> Result<(), MusicBotError> {
    Ok(())
}

/// Gather everyone in your voice channel — they tap "I'm here!" to check in.
#[poise::command(slash_command, prefix_command, check = "check_author_in_voice_channel")]
pub async fn start(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let guild_id: GuildId = ctx.guild_id().ok_or(MusicBotError::NoGuildIdError)?;

    let voice_channel_id: ChannelId =
        match channel_service::get_user_voice_channel(ctx, &ctx.author().id) {
            Some(c) => c,
            None => {
                BotEmbed::CurrentUserNotInVoiceChannel
                    .to_embed()
                    .send_context(ctx, true, Some(30))
                    .await?;
                return Ok(());
            }
        };

    {
        let gatherings = ctx.data().gatherings.read().await;
        if gatherings.contains_key(&guild_id) {
            CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Gathering already running")
                .description("There's already an active gathering in this guild.")
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    }

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

    let state = Arc::new(GatherState::default());

    ctx.data()
        .gatherings
        .write()
        .await
        .insert(guild_id, Arc::clone(&state));

    let result = gather_service::start_gather(
        ctx.serenity_context(),
        guild_id,
        ctx.channel_id(),
        voice_channel_id,
        ctx.author().id,
        state,
    )
    .await;

    ctx.data().gatherings.write().await.remove(&guild_id);

    result
}

/// Add a user to wait for — gathering won't finish until they check in too.
#[poise::command(slash_command, prefix_command)]
pub async fn expect(
    ctx: Context<'_>,
    #[description = "User to wait for"] user: User,
    #[description = "Suppress ghost-ping reminders for this user (default: pings enabled)"]
    silent: Option<bool>,
) -> Result<(), MusicBotError> {
    let guild_id = ctx.guild_id().ok_or(MusicBotError::NoGuildIdError)?;

    let state = {
        let gatherings = ctx.data().gatherings.read().await;
        gatherings.get(&guild_id).cloned()
    };

    let state = match state {
        Some(s) => s,
        None => {
            CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  No active gathering")
                .description(
                    "There's no gathering running right now. Start one with `/gather start`.",
                )
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    };

    state.extra_expected.lock().unwrap().insert(user.id);

    let silent = silent.unwrap_or(false);
    if silent {
        state.no_spam.lock().unwrap().insert(user.id);
    }

    let desc = if silent {
        format!(
            "{} added to the gathering.\nGhost-ping reminders are **disabled** for them.",
            user.mention()
        )
    } else {
        format!("{} added to the gathering.", user.mention())
    };

    CreateEmbed::new()
        .color(Color::DARK_GREEN)
        .title("✅  User expected")
        .description(desc)
        .send_context(ctx, true, Some(15))
        .await?;

    Ok(())
}
