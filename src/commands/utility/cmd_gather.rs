use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::embeds::bot_embeds::BotEmbed;
use crate::embeds::gather_embed::GatherEmbed;
use crate::player::notifier::{get_current_time, parse_duration_from_string};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::gather_service::{self, GatherState, PREGATHER_DURATION};
use crate::service::utils_service::humanize_duration;
use serenity::all::{
    ChannelId, CreateInteractionResponse, CreateInteractionResponseMessage, GuildId, Mentionable,
    User,
};
use std::sync::Arc;
use std::time::Duration;

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
pub async fn start(
    ctx: Context<'_>,
    #[description = "Pre-gather countdown length (e.g. 30s, 2m). Default: 1 minute."]
    time: Option<String>,
) -> Result<(), MusicBotError> {
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

    let (pregather_duration, schedule_label) = match time {
        Some(ref t) => match parse_pregather_time(t.trim()) {
            Some(pair) => pair,
            None => {
                GatherEmbed::InvalidPregatherTime
                    .to_embed()
                    .send_context(ctx, true, Some(15))
                    .await?;
                return Ok(());
            }
        },
        None => (PREGATHER_DURATION, format!("in {}", humanize_duration(PREGATHER_DURATION))),
    };

    {
        let gatherings = ctx.data().gatherings.read().await;
        if gatherings.contains_key(&guild_id) {
            GatherEmbed::AlreadyRunning
                .to_embed()
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

    let state = Arc::new(GatherState::new(voice_channel_id));

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
        ctx.author().mention().to_string(),
        schedule_label,
        state,
        pregather_duration,
    )
    .await;

    ctx.data().gatherings.write().await.remove(&guild_id);

    result
}

/// Add users to wait for — gathering won't finish until they all check in too.
#[poise::command(slash_command, prefix_command)]
pub async fn expect(
    ctx: Context<'_>,
    #[description = "User to wait for"] user1: User,
    #[description = "Second user to wait for"] user2: Option<User>,
    #[description = "Third user to wait for"] user3: Option<User>,
    #[description = "Fourth user to wait for"] user4: Option<User>,
    #[description = "Fifth user to wait for"] user5: Option<User>,
) -> Result<(), MusicBotError> {
    let guild_id = ctx.guild_id().ok_or(MusicBotError::NoGuildIdError)?;

    let state = {
        let gatherings = ctx.data().gatherings.read().await;
        gatherings.get(&guild_id).cloned()
    };

    let state = match state {
        Some(s) => s,
        None => {
            GatherEmbed::NoActiveGathering
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    };

    let users: Vec<&User> = [
        Some(&user1),
        user2.as_ref(),
        user3.as_ref(),
        user4.as_ref(),
        user5.as_ref(),
    ]
    .into_iter()
    .flatten()
    .collect();

    {
        let mut extra = state.extra_expected.lock().unwrap();
        for u in &users {
            extra.insert(u.id);
        }
    }

    let names = users
        .iter()
        .map(|u| u.mention().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    GatherEmbed::UsersExpected { names: &names }
        .to_embed()
        .send_context(ctx, true, Some(15))
        .await?;

    Ok(())
}

/// Parse a pregather countdown from either a relative duration (`10m`, `1h 30m`)
/// or a clock time (`10:00`, `14:30`). Returns `(duration, display_label)` where
/// the label is "in X minutes" for relative or "at HH:MM" for clock times.
fn parse_pregather_time(text: &str) -> Option<(Duration, String)> {
    // Relative duration: 10m, 1h, 30s, 1h 30m, …
    if let Some(d) = parse_duration_from_string(text) {
        let label = format!("in {}", humanize_duration(d));
        return Some((d, label));
    }

    // Clock time: HH:MM or H:MM
    let (h_str, m_str) = text.split_once(':')?;
    let hour: u8 = h_str.trim().parse().ok()?;
    let minute: u8 = m_str.trim().parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }

    let now = get_current_time();
    let now_secs = now.hour() as u64 * 3600 + now.minute() as u64 * 60 + now.second() as u64;
    let target_secs = hour as u64 * 3600 + minute as u64 * 60;

    let until_secs = if target_secs > now_secs {
        target_secs - now_secs
    } else {
        // Time has already passed today — target tomorrow.
        86400 - now_secs + target_secs
    };

    if until_secs == 0 {
        return None;
    }

    let label = format!("at {:02}:{:02}", hour, minute);
    Some((Duration::from_secs(until_secs), label))
}
