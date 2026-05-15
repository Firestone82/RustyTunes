use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::embeds::activity::break_embed::BreakEmbed;
use crate::embeds::bot::bot_embeds::BotEmbed;
use crate::service::break_service::{self, parse_break_duration, parse_break_start_time, BreakState, MAX_BREAK_DURATION};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::gather_service::{self, GatherState};
use crate::utils::time_utils::get_current_time;
use serenity::all::{ChannelId, CreateInteractionResponse, CreateInteractionResponseMessage, GuildId, Mentionable, UserId};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Take a break — when the timer runs out, everyone in voice is auto-gathered.
#[poise::command(
    slash_command,
    prefix_command,
    subcommands("start", "extend"),
    subcommand_required
)]
pub async fn r#break(_ctx: Context<'_>) -> Result<(), MusicBotError> {
    Ok(())
}

/// Start a break of the given length. When the timer ends, a gathering is
/// kicked off automatically.
#[poise::command(
    slash_command,
    prefix_command,
    check = "check_author_in_voice_channel"
)]
pub async fn start(
    ctx: Context<'_>,
    #[description = "Break end time or duration, e.g. `5m`, `1h 30s`, `14:00`."] time: String,
) -> Result<(), MusicBotError> {
    let (duration, clock_time_label) = match parse_break_start_time(&time) {
        Some((d, label)) if d > Duration::ZERO && d <= MAX_BREAK_DURATION => (d, label),
        Some(_) => {
            BreakEmbed::TooLong { max: MAX_BREAK_DURATION }
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
        None => {
            BreakEmbed::InvalidDuration
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    };

    let guild_id: GuildId = ctx.guild_id().ok_or(MusicBotError::NoGuildIdError)?;
    let author_id: UserId = ctx.author().id;

    let voice_channel_id: ChannelId = match channel_service::get_user_voice_channel(ctx, &author_id) {
        Some(c) => c,
        None => {
            BotEmbed::CurrentUserNotInVoiceChannel
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
            return Ok(());
        }
    };

    // Refuse to start a second break if one is already running in this guild.
    {
        let breaks = ctx.data().breaks.read().await;
        if breaks.contains_key(&guild_id) {
            BreakEmbed::AlreadyRunning
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    }

    if let poise::Context::Application(app_ctx) = ctx {
        let _ = app_ctx
            .interaction
            .create_response(
                ctx.http(),
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Starting break…")
                        .ephemeral(true),
                ),
            )
            .await;
    }

    let text_channel_id: ChannelId = ctx.channel_id();
    let author_mention = ctx.author().mention().to_string();

    let state = Arc::new(BreakState {
        author_id,
        started_at_instant: Instant::now(),
        started_at_wall: get_current_time(),
        original_duration: duration,
        extension: Mutex::new(Duration::ZERO),
        author_mention: author_mention.clone(),
        clock_time_label,
    });

    ctx.data()
        .breaks
        .write()
        .await
        .insert(guild_id, Arc::clone(&state));

    let cancelled = break_service::run_break(
        ctx.serenity_context(),
        guild_id,
        text_channel_id,
        voice_channel_id,
        Arc::clone(&state),
    )
    .await?;

    // Drop the break state before handing off to the gather flow.
    ctx.data().breaks.write().await.remove(&guild_id);

    if cancelled {
        return Ok(());
    }

    let gather_state = Arc::new(GatherState::new(voice_channel_id));
    ctx.data()
        .gatherings
        .write()
        .await
        .insert(guild_id, Arc::clone(&gather_state));

    // Skip the pre-gather countdown — break already announced and pinged everyone.
    gather_service::start_gather(
        ctx.serenity_context(),
        guild_id,
        text_channel_id,
        voice_channel_id,
        author_id,
        author_mention,
        String::new(),
        gather_state,
        Duration::ZERO,
    )
    .await?;

    ctx.data().gatherings.write().await.remove(&guild_id);

    Ok(())
}

/// Extend the current break by the given amount of time.
#[poise::command(slash_command, prefix_command)]
pub async fn extend(
    ctx: Context<'_>,
    #[description = "Extra time to add, e.g. `5m`, `30s`, `1h 30s`."] time: String,
) -> Result<(), MusicBotError> {
    let extra = match parse_break_duration(&time) {
        Some(d) if d > Duration::ZERO => d,
        _ => {
            BreakEmbed::InvalidExtension
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    };

    let guild_id: GuildId = ctx.guild_id().ok_or(MusicBotError::NoGuildIdError)?;

    let state = {
        let breaks = ctx.data().breaks.read().await;
        breaks.get(&guild_id).cloned()
    };

    let state = match state {
        Some(s) => s,
        None => {
            BreakEmbed::NoActiveBreak
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    };

    let new_total = state.total_duration() + extra;
    if new_total > MAX_BREAK_DURATION {
        BreakEmbed::ExceedsCap { new_total, cap: MAX_BREAK_DURATION }
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await?;
        return Ok(());
    }

    {
        let mut ext = state.extension.lock().unwrap();
        *ext += extra;
    }

    let author_mention = ctx.author().mention().to_string();
    BreakEmbed::Extended {
        author_mention: &author_mention,
        extra,
        total: state.total_duration(),
        ends_at: state.ends_at_wall(),
    }
    .to_embed()
    .send_context(ctx, false, None)
    .await?;

    Ok(())
}
