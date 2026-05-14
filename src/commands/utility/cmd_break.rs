use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::embeds::bot_embeds::BotEmbed;
use crate::player::notifier::{convert_time_offset_from_string, get_current_time};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::gather_service;
use serenity::all::{
    ButtonStyle, ChannelId, Color, CreateActionRow, CreateButton, CreateEmbed,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditMessage,
    GuildId, Mentionable, Message, UserId,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

const BTN_BREAK_CANCEL: &str = "break_cancel";
const MAX_BREAK_DURATION: Duration = Duration::from_secs(60 * 60 * 4);
const MIN_EDIT_INTERVAL: Duration = Duration::from_secs(5);

/// Shared mutable state for the active break in a guild. Mutated by
/// `/break extend` and read by the running `/break start` loop.
pub struct BreakState {
    pub author_id: UserId,
    pub started_at_instant: Instant,
    pub started_at_wall: OffsetDateTime,
    pub original_duration: Duration,
    pub extension: Mutex<Duration>,
    pub author_mention: String,
}

impl BreakState {
    fn total_duration(&self) -> Duration {
        self.original_duration + *self.extension.lock().unwrap()
    }

    fn ends_at_instant(&self) -> Instant {
        self.started_at_instant + self.total_duration()
    }

    fn ends_at_wall(&self) -> OffsetDateTime {
        self.started_at_wall + self.total_duration()
    }

    fn extension_total(&self) -> Duration {
        *self.extension.lock().unwrap()
    }
}

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
#[poise::command(slash_command, prefix_command, check = "check_author_in_voice_channel")]
pub async fn start(
    ctx: Context<'_>,
    #[description = "Break length, e.g. `5m`, `1h 30s`, `90s`."] time: String,
) -> Result<(), MusicBotError> {
    let duration = match parse_break_duration(&time) {
        Some(d) if d > Duration::ZERO && d <= MAX_BREAK_DURATION => d,
        Some(_) => {
            CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Break too long")
                .description(format!(
                    "Maximum break length is {}.",
                    humanize_duration(MAX_BREAK_DURATION)
                ))
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
        None => {
            CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Invalid break duration")
                .description("Use a relative duration like `5m`, `1h 30s`, or `90s`.")
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    };

    let guild_id: GuildId = ctx.guild_id().ok_or(MusicBotError::NoGuildIdError)?;
    let author_id: UserId = ctx.author().id;

    let voice_channel_id: ChannelId =
        match channel_service::get_user_voice_channel(ctx, &author_id) {
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
            CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Break already running")
                .description("There's already an active break in this guild — extend it with `/break extend <time>` instead.")
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
    });

    ctx.data()
        .breaks
        .write()
        .await
        .insert(guild_id, Arc::clone(&state));

    // Ping all voice members in a separate message above the embed.
    let bot_id = ctx.serenity_context().cache.current_user().id;
    let voice_mentions: String = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .as_ref()
        .map(|g| {
            g.voice_states
                .values()
                .filter(|vs| vs.channel_id == Some(voice_channel_id) && vs.user_id != bot_id)
                .map(|vs| vs.user_id.mention().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if !voice_mentions.is_empty() {
        let _ = text_channel_id
            .send_message(&ctx.http(), CreateMessage::new().content(voice_mentions))
            .await;
    }

    let mut msg: Message = text_channel_id
        .send_message(
            &ctx.http(),
            CreateMessage::new()
                .embed(build_break_embed(&state, None))
                .components(break_buttons(false)),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let mut cancelled = false;
    let mut last_edit = Instant::now();
    let shard = ctx.serenity_context().shard.clone();

    loop {
        let now = Instant::now();
        let ends_at = state.ends_at_instant();
        if now >= ends_at {
            break;
        }

        let remaining = ends_at.saturating_duration_since(now);
        let wait = remaining.min(MIN_EDIT_INTERVAL);

        let interaction = msg
            .await_component_interaction(shard.clone())
            .timeout(wait)
            .await;

        if let Some(ic) = interaction {
            if ic.data.custom_id == BTN_BREAK_CANCEL {
                if ic.user.id != author_id {
                    ic.create_response(
                        ctx.http(),
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Only the person who started the break can cancel it.")
                                .ephemeral(true),
                        ),
                    )
                    .await
                    .ok();
                    continue;
                }
                ic.create_response(ctx.http(), CreateInteractionResponse::Acknowledge)
                    .await
                    .ok();
                cancelled = true;
                break;
            }
        }

        if Instant::now() < last_edit + MIN_EDIT_INTERVAL {
            continue;
        }
        last_edit = Instant::now();
        let _ = msg
            .edit(
                ctx.http(),
                EditMessage::new()
                    .embed(build_break_embed(&state, None))
                    .components(break_buttons(false)),
            )
            .await;
    }

    let footer = if cancelled {
        "Break cancelled."
    } else {
        "Break is over — starting gathering."
    };

    let _ = msg
        .edit(
            ctx.http(),
            EditMessage::new()
                .embed(build_break_embed(&state, Some(footer)))
                .components(Vec::new()),
        )
        .await;

    // Drop the state before handing off to the gather flow.
    ctx.data().breaks.write().await.remove(&guild_id);

    if cancelled {
        return Ok(());
    }

    gather_service::start_gather(
        ctx.serenity_context(),
        guild_id,
        text_channel_id,
        voice_channel_id,
        author_id,
    )
    .await?;

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
            CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Invalid extension")
                .description("Use a relative duration like `5m`, `1h 30s`, or `90s`.")
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
            CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  No active break")
                .description("There's no break running right now. Start one with `/break start <time>`.")
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }
    };

    let new_total = state.total_duration() + extra;
    if new_total > MAX_BREAK_DURATION {
        CreateEmbed::new()
            .color(Color::DARK_RED)
            .title("🚫  Extension would exceed cap")
            .description(format!(
                "Total break length would be `{}`, over the {} cap.",
                humanize_duration(new_total),
                humanize_duration(MAX_BREAK_DURATION)
            ))
            .send_context(ctx, true, Some(15))
            .await?;
        return Ok(());
    }

    {
        let mut ext = state.extension.lock().unwrap();
        *ext += extra;
    }

    let new_end = state.ends_at_wall();
    CreateEmbed::new()
        .color(Color::DARK_GREEN)
        .title("⏱️  Break extended")
        .description(format!(
            "{} extended the break by **{}**.\n\n\
             New total: **{}**\n\
             Ends at: `{}`",
            ctx.author().mention(),
            humanize_duration(extra),
            humanize_duration(state.total_duration()),
            format_wall_clock(new_end),
        ))
        .send_context(ctx, false, None)
        .await?;

    Ok(())
}

fn parse_break_duration(text: &str) -> Option<Duration> {
    let target = convert_time_offset_from_string(text.trim().to_string())?;
    let now = get_current_time();
    let secs = (target - now).whole_seconds();
    if secs <= 0 {
        return None;
    }
    Some(Duration::from_secs(secs as u64))
}

fn break_buttons(disabled: bool) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![CreateButton::new(
        BTN_BREAK_CANCEL,
    )
    .label("Cancel")
    .style(ButtonStyle::Danger)
    .disabled(disabled)])]
}

fn build_break_embed(state: &BreakState, footer: Option<&str>) -> CreateEmbed {
    let now = Instant::now();
    let ends_at = state.ends_at_instant();
    let total = state.total_duration();
    let remaining = ends_at.saturating_duration_since(now);
    let extension = state.extension_total();

    let color = if footer.is_some() {
        Color::DARK_GREEN
    } else {
        Color::DARK_GOLD
    };

    let mut description = format!(
        "{} started a break of **{}**.\n\n\
         Time remaining: **{}**\n\
         Ends at: `{}`",
        state.author_mention,
        humanize_duration(state.original_duration),
        humanize_duration(remaining),
        format_wall_clock(state.ends_at_wall()),
    );

    if extension > Duration::ZERO {
        description.push_str(&format!(
            "\nExtended by: **{}** (total **{}**)",
            humanize_duration(extension),
            humanize_duration(total),
        ));
    }

    description.push_str(
        "\n\nWhen the timer ends, everyone still in voice will be gathered \
         automatically — late arrivals will be tracked.",
    );

    let mut builder = CreateEmbed::new()
        .color(color)
        .title("⏸️  Break in progress")
        .description(description);

    if let Some(text) = footer {
        builder = builder.footer(serenity::all::CreateEmbedFooter::new(text));
    }

    builder
}

fn humanize_duration(d: Duration) -> String {
    let total = d.as_secs();
    if total == 0 {
        return "0 seconds".to_string();
    }

    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;

    let mut parts: Vec<String> = Vec::new();
    if h > 0 {
        parts.push(format!("{} {}", h, if h == 1 { "hour" } else { "hours" }));
    }
    if m > 0 {
        parts.push(format!("{} {}", m, if m == 1 { "minute" } else { "minutes" }));
    }
    if s > 0 {
        parts.push(format!("{} {}", s, if s == 1 { "second" } else { "seconds" }));
    }
    parts.join(" ")
}

fn format_wall_clock(t: OffsetDateTime) -> String {
    format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second())
}
