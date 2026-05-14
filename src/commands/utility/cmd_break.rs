use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_voice_channel;
use crate::embeds::bot_embeds::BotEmbed;
use crate::player::notifier::convert_time_offset_from_string;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::gather_service;
use serenity::all::{
    ButtonStyle, ChannelId, Color, CreateActionRow, CreateButton, CreateEmbed,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditMessage,
    GuildId, Mentionable, Message, UserId,
};
use std::time::{Duration, Instant};

const BTN_BREAK_CANCEL: &str = "break_cancel";
const MAX_BREAK_DURATION: Duration = Duration::from_secs(60 * 60 * 4);
const MIN_EDIT_INTERVAL: Duration = Duration::from_secs(5);

/// Take a break — when the timer runs out, everyone in voice is auto-gathered.
#[poise::command(slash_command, prefix_command, check = "check_author_in_voice_channel")]
pub async fn r#break(
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
                    format_hhmmss(MAX_BREAK_DURATION)
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

    // Acknowledge the slash command — the timer message itself is a normal
    // channel message so it can outlive the interaction token.
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
    let started_at = Instant::now();
    let ends_at = started_at + duration;
    let author_mention = ctx.author().mention().to_string();

    let mut msg: Message = text_channel_id
        .send_message(
            &ctx.http(),
            CreateMessage::new()
                .content(format!(
                    "@here  ⏸️  {} started a break.",
                    author_mention
                ))
                .embed(build_break_embed(
                    &author_mention,
                    duration,
                    Instant::now(),
                    ends_at,
                    None,
                ))
                .components(break_buttons(false)),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let mut cancelled = false;
    let mut last_edit = Instant::now();
    let shard = ctx.serenity_context().shard.clone();

    loop {
        let now = Instant::now();
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
                    .embed(build_break_embed(
                        &author_mention,
                        duration,
                        Instant::now(),
                        ends_at,
                        None,
                    ))
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
                .embed(build_break_embed(
                    &author_mention,
                    duration,
                    Instant::now(),
                    ends_at,
                    Some(footer),
                ))
                .components(Vec::new()),
        )
        .await;

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

fn parse_break_duration(text: &str) -> Option<Duration> {
    let target = convert_time_offset_from_string(text.trim().to_string())?;
    let now = crate::player::notifier::get_current_time();
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

fn build_break_embed(
    author_mention: &str,
    total: Duration,
    now: Instant,
    ends_at: Instant,
    footer: Option<&str>,
) -> CreateEmbed {
    let remaining = ends_at.saturating_duration_since(now);
    let color = if footer.is_some() {
        Color::DARK_GREEN
    } else {
        Color::DARK_GOLD
    };

    let mut builder = CreateEmbed::new()
        .color(color)
        .title("⏸️  Break in progress")
        .description(format!(
            "{} started a break of `{}`.\n\nTime remaining: **{}**\n\n\
             When the timer ends, everyone still in voice will be gathered \
             automatically — late arrivals will be tracked.",
            author_mention,
            format_hhmmss(total),
            format_hhmmss(remaining),
        ));

    if let Some(text) = footer {
        builder = builder.footer(serenity::all::CreateEmbedFooter::new(text));
    }

    builder
}

fn format_hhmmss(d: Duration) -> String {
    let total = d.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}
