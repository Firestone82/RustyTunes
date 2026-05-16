use crate::bot::MusicBotError;
use crate::embeds::activity::break_embed::{break_buttons, BreakEmbed, BTN_BREAK_CANCEL, BTN_BREAK_SKIP};
use crate::service::attendance_service;
use crate::utils::time_utils::{get_current_time, parse_duration_from_string};
use serenity::all::{ChannelId, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditMessage, GuildId, Mentionable, Message, UserId};
use serenity::prelude::Context as SerenityContext;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

pub const MAX_BREAK_DURATION: Duration = Duration::from_secs(60 * 60 * 4);
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
    /// Set when the break was started with a clock time (e.g. "17:10") rather
    /// than a relative duration. Controls the embed wording.
    pub clock_time_label: Option<String>,
    /// Users added via `/break expect` — forwarded to the gathering started when the break ends.
    pub extra_expected: Mutex<HashSet<UserId>>,
    /// Users removed via `/break forget` — forwarded to the gathering so they're dropped from expectations.
    pub forgotten: Mutex<HashSet<UserId>>,
}

impl BreakState {
    pub fn total_duration(&self) -> Duration {
        self.original_duration + *self.extension.lock().unwrap()
    }

    pub fn ends_at_instant(&self) -> Instant {
        self.started_at_instant + self.total_duration()
    }

    pub fn ends_at_wall(&self) -> OffsetDateTime {
        self.started_at_wall + self.total_duration()
    }

    pub fn extension_total(&self) -> Duration {
        *self.extension.lock().unwrap()
    }
}

/// Runs the break timer for an existing `BreakState`. Returns `Ok(true)` if the
/// break was cancelled (so the caller should skip auto-gather), `Ok(false)` if
/// it ran to completion.
pub async fn run_break(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    text_channel_id: ChannelId,
    voice_channel_id: ChannelId,
    state: Arc<BreakState>,
) -> Result<bool, MusicBotError> {
    let bot_id = serenity_ctx.cache.current_user().id;

    // Voice members are pinged in the embed message's content field so the
    // @mentions stay glued to the embed (separate messages above tended to get
    // visually orphaned as the break embed refreshed).
    let voice_mentions: String = serenity_ctx
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

    let mut msg: Message = text_channel_id
        .send_message(
            &serenity_ctx.http,
            CreateMessage::new()
                .content(voice_mentions)
                .embeds(break_message_embeds(
                    serenity_ctx,
                    guild_id,
                    voice_channel_id,
                    &state,
                    None,
                ))
                .components(break_buttons(false)),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let mut cancelled = false;
    let mut last_edit = Instant::now();
    let shard = serenity_ctx.shard.clone();

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
            match ic.data.custom_id.as_str() {
                BTN_BREAK_CANCEL | BTN_BREAK_SKIP => {
                    if ic.user.id != state.author_id {
                        ic.create_response(
                            &serenity_ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Only the person who started the break can do that.")
                                    .ephemeral(true),
                            ),
                        )
                        .await
                        .ok();
                        continue;
                    }
                    ic.create_response(&serenity_ctx.http, CreateInteractionResponse::Acknowledge)
                        .await
                        .ok();
                    if ic.data.custom_id == BTN_BREAK_CANCEL {
                        cancelled = true;
                    }
                    break;
                }
                _ => {}
            }
        }

        if Instant::now() < last_edit + MIN_EDIT_INTERVAL {
            continue;
        }
        last_edit = Instant::now();
        let _ = msg
            .edit(
                &serenity_ctx.http,
                EditMessage::new()
                    .embeds(break_message_embeds(
                        serenity_ctx,
                        guild_id,
                        voice_channel_id,
                        &state,
                        None,
                    ))
                    .components(break_buttons(false)),
            )
            .await;
    }

    let footer = if cancelled { "Break cancelled." } else { "Break is over — starting gathering." };

    let _ = msg
        .edit(
            &serenity_ctx.http,
            EditMessage::new()
                .embeds(break_message_embeds(
                    serenity_ctx,
                    guild_id,
                    voice_channel_id,
                    &state,
                    Some(footer),
                ))
                .components(Vec::new()),
        )
        .await;

    Ok(cancelled)
}

/// Returns the embed pair posted on the break message: the countdown
/// progress embed, followed by the live attendee list.
fn break_message_embeds(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    state: &BreakState,
    footer: Option<&str>,
) -> Vec<CreateEmbed> {
    let main = progress_embed(state, footer, expected_mentions_text(state).as_deref());
    let extra = state.extra_expected.lock().unwrap().clone();
    let forgotten = state.forgotten.lock().unwrap().clone();
    let attendees = attendance_service::attendees_embed(serenity_ctx, guild_id, voice_channel_id, &extra, &forgotten);
    vec![main, attendees]
}

fn progress_embed(
    state: &BreakState,
    footer: Option<&str>,
    expected_mentions: Option<&str>,
) -> CreateEmbed {
    let now = Instant::now();
    let ends_at = state.ends_at_instant();
    let remaining = ends_at.saturating_duration_since(now);
    let total = state.total_duration();
    let extension = state.extension_total();

    BreakEmbed::Progress {
        author_mention: &state.author_mention,
        clock_time_label: state.clock_time_label.as_deref(),
        original_duration: state.original_duration,
        remaining,
        extension,
        total,
        expected_mentions,
        footer,
    }
    .to_embed()
}

/// Comma-separated mentions of users `/break expect` has queued, or `None` if empty.
pub fn expected_mentions_text(state: &BreakState) -> Option<String> {
    let extra = state.extra_expected.lock().unwrap();
    if extra.is_empty() {
        return None;
    }
    Some(
        extra
            .iter()
            .map(|id| id.mention().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Parse a break start time: relative duration (`5m`, `1h 30s`) or clock time
/// (`14:00`). Returns `(duration, clock_label)` — `clock_label` is `Some("17:10")`
/// when a clock time was supplied, `None` for relative durations.
pub fn parse_break_start_time(text: &str) -> Option<(Duration, Option<String>)> {
    let text = text.trim();

    // Relative duration: 10m, 1h, 30s, 1h 30m, …
    if let Some(d) = parse_duration_from_string(text) {
        if d == Duration::ZERO {
            return None;
        }
        return Some((d, None));
    }

    // Clock time: HH:MM or H:MM — break ends at that wall-clock time.
    let (h_str, m_str) = text.split_once(':')?;
    let hour: u8 = h_str.trim().parse().ok()?;
    let minute: u8 = m_str.trim().parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }

    let now = get_current_time();
    let now_secs = now.hour() as u64 * 3600 + now.minute() as u64 * 60 + now.second() as u64;
    let target_secs = hour as u64 * 3600 + minute as u64 * 60;

    let until_secs = if target_secs > now_secs { target_secs - now_secs } else { 86400 - now_secs + target_secs };

    if until_secs == 0 {
        return None;
    }

    let label = format!("{:02}:{:02}", hour, minute);
    Some((Duration::from_secs(until_secs), Some(label)))
}

/// Parse a relative-only break extension: `5m`, `1h 30s`, `90s`, etc.
/// Clock times are intentionally not supported here — an extension must be an
/// amount of additional time, not an absolute end time.
pub fn parse_break_duration(text: &str) -> Option<Duration> {
    let d = parse_duration_from_string(text.trim())?;
    if d == Duration::ZERO {
        return None;
    }
    Some(d)
}
