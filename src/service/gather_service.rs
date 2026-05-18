use crate::bot::MusicBotError;
use crate::embeds::activity::gather_embed::{
    gather_buttons, pregather_buttons, CheckInRow, GatherEmbed, BTN_CANCEL, BTN_FORCE_START, BTN_HERE, BTN_JOIN, BTN_LEAVE, BTN_NOT_COMING, BTN_TOGGLE_SILENT, GRACE_PERIOD,
};
use crate::service::attendance_service;
use crate::service::interaction_service::DeferredInteractionStream;
use crate::utils::string_utils::sanitize_name;
use crate::utils::time_utils::get_current_time;
use serenity::all::{ChannelId, ComponentInteraction, CreateEmbed, CreateInteractionResponseFollowup, CreateMessage, EditMessage, GuildId, Mentionable, Message, UserId};
use serenity::http::Http;
use serenity::prelude::Context as SerenityContext;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// Tracks the running pre-gather countdown. `None` once the check-in phase
/// has started or the countdown was skipped (e.g. break → gather hand-off),
/// so `/gather extend` knows whether there's still a countdown to grow.
pub struct PregatherInfo {
    pub started_at: Instant,
    pub started_at_wall: OffsetDateTime,
    pub original_duration: Duration,
}

/// Shared state for an active gathering.
pub struct GatherState {
    /// The voice channel this gathering is tracking.
    pub voice_channel_id: ChannelId,
    /// Users added via `/gather expect` that must check in before gathering ends.
    pub extra_expected: Mutex<HashSet<UserId>>,
    /// Users removed via `/gather forget` — drained by the check-in loop to drop
    /// them from the `expected` working set (unless they've already arrived).
    pub forgotten: Mutex<HashSet<UserId>>,
    /// Users who joined the gathering voice channel while expected — processed on the next loop tick.
    pub auto_arrived: Mutex<HashSet<UserId>>,
    /// When true, ghost-ping reminders are suppressed for everyone.
    pub silent: Mutex<bool>,
    /// Set while the pre-gather countdown is active; cleared once it ends.
    pub pregather: Mutex<Option<PregatherInfo>>,
    /// Total time `/gather extend` has added to the pre-gather countdown.
    pub pregather_extension: Mutex<Duration>,
}

impl GatherState {
    pub fn new(voice_channel_id: ChannelId) -> Self {
        Self {
            voice_channel_id,
            extra_expected: Mutex::new(HashSet::new()),
            forgotten: Mutex::new(HashSet::new()),
            auto_arrived: Mutex::new(HashSet::new()),
            silent: Mutex::new(false),
            pregather: Mutex::new(None),
            pregather_extension: Mutex::new(Duration::ZERO),
        }
    }
}

pub const PREGATHER_DURATION: Duration = Duration::from_secs(60);
pub const MAX_PREGATHER_DURATION: Duration = Duration::from_secs(60 * 60 * 2);
const GHOST_PING_INTERVAL: Duration = Duration::from_secs(30);
const MAX_GATHER_DURATION: Duration = Duration::from_secs(60 * 30);
const GHOST_PING_LIFETIME: Duration = Duration::from_millis(700);
const MIN_EDIT_INTERVAL: Duration = Duration::from_secs(5);
// Max wait in the select loop — keeps button response latency well within 3 s.
const LOOP_POLL: Duration = Duration::from_millis(800);

/// What the interaction handler decided should happen next.
enum CheckInOutcome {
    /// State unchanged or ephemeral-only response — no need to re-render.
    None,
    /// State changed — main loop should re-render the embed.
    Refresh,
    /// Author hit cancel — main loop should exit.
    Cancel,
}

pub async fn start_gather(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    text_channel_id: ChannelId,
    voice_channel_id: ChannelId,
    author_id: UserId,
    author_mention: String,
    schedule_label: String,
    state: Arc<GatherState>,
    pregather_duration: Duration,
) -> Result<(), MusicBotError> {
    let bot_id = serenity_ctx.cache.current_user().id;

    let initial_voice_ids: Vec<UserId> = current_voice_members(serenity_ctx, guild_id, voice_channel_id, bot_id);
    if initial_voice_ids.is_empty() {
        return Err(MusicBotError::InternalError(
            "No one is in the voice channel.".to_string(),
        ));
    }

    // ── Phase 1: pre-gather countdown (skipped when pregather_duration == 0,
    // i.e. auto-gather right after a break).
    let mut msg: Message;

    if pregather_duration > Duration::ZERO {
        // Ping voice members in a separate message above the embed so the
        // gather message itself stays a clean single embed.
        let voice_mentions: String = initial_voice_ids
            .iter()
            .map(|id| id.mention().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let _ = text_channel_id
            .send_message(
                &serenity_ctx.http,
                CreateMessage::new().content(voice_mentions),
            )
            .await;

        let pregather_started_at = Instant::now();
        let pregather_started_at_wall = get_current_time();
        *state.pregather.lock().unwrap() = Some(PregatherInfo {
            started_at: pregather_started_at,
            started_at_wall: pregather_started_at_wall,
            original_duration: pregather_duration,
        });

        msg = text_channel_id
            .send_message(
                &serenity_ctx.http,
                CreateMessage::new()
                    .embeds(pregather_message_embeds(
                        serenity_ctx,
                        guild_id,
                        voice_channel_id,
                        &state,
                        pregather_started_at,
                        pregather_started_at_wall,
                        pregather_duration,
                        &author_mention,
                        &schedule_label,
                        None,
                    ))
                    .components(pregather_buttons(false)),
            )
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

        let mut stream = DeferredInteractionStream::new(serenity_ctx, msg.id);

        let pregather_cancelled = 'pregather: loop {
            let now = Instant::now();
            let ends_at = pregather_started_at + pregather_duration + *state.pregather_extension.lock().unwrap();
            if now >= ends_at {
                break false;
            }

            let wait = ends_at
                .saturating_duration_since(now)
                .min(MIN_EDIT_INTERVAL);

            match stream.next_within(wait).await {
                Some(ic) => match ic.data.custom_id.as_str() {
                    BTN_CANCEL => {
                        if ic.user.id != author_id {
                            send_ephemeral(
                                &serenity_ctx.http,
                                &ic,
                                "Only the person who started the gathering can cancel it.",
                            )
                            .await;
                            continue 'pregather;
                        }
                        break 'pregather true;
                    }
                    BTN_FORCE_START => {
                        if ic.user.id != author_id {
                            send_ephemeral(
                                &serenity_ctx.http,
                                &ic,
                                "Only the person who started the gathering can skip the countdown.",
                            )
                            .await;
                            continue 'pregather;
                        }
                        break 'pregather false;
                    }
                    BTN_JOIN => {
                        {
                            let mut extra = state.extra_expected.lock().unwrap();
                            let mut forgotten = state.forgotten.lock().unwrap();
                            extra.insert(ic.user.id);
                            forgotten.remove(&ic.user.id);
                        }
                        let _ = msg
                            .edit(
                                &serenity_ctx.http,
                                EditMessage::new()
                                    .embeds(pregather_message_embeds(
                                        serenity_ctx,
                                        guild_id,
                                        voice_channel_id,
                                        &state,
                                        pregather_started_at,
                                        pregather_started_at_wall,
                                        pregather_duration,
                                        &author_mention,
                                        &schedule_label,
                                        None,
                                    ))
                                    .components(pregather_buttons(false)),
                            )
                            .await;
                    }
                    BTN_LEAVE => {
                        {
                            let mut extra = state.extra_expected.lock().unwrap();
                            let mut forgotten = state.forgotten.lock().unwrap();
                            extra.remove(&ic.user.id);
                            forgotten.insert(ic.user.id);
                        }
                        let _ = msg
                            .edit(
                                &serenity_ctx.http,
                                EditMessage::new()
                                    .embeds(pregather_message_embeds(
                                        serenity_ctx,
                                        guild_id,
                                        voice_channel_id,
                                        &state,
                                        pregather_started_at,
                                        pregather_started_at_wall,
                                        pregather_duration,
                                        &author_mention,
                                        &schedule_label,
                                        None,
                                    ))
                                    .components(pregather_buttons(false)),
                            )
                            .await;
                    }
                    _ => {}
                },
                None => {
                    // Timeout: refresh the countdown display (also reflects /gather extend, /gather expect, /gather forget).
                    let _ = msg
                        .edit(
                            &serenity_ctx.http,
                            EditMessage::new()
                                .embeds(pregather_message_embeds(
                                    serenity_ctx,
                                    guild_id,
                                    voice_channel_id,
                                    &state,
                                    pregather_started_at,
                                    pregather_started_at_wall,
                                    pregather_duration,
                                    &author_mention,
                                    &schedule_label,
                                    None,
                                ))
                                .components(pregather_buttons(false)),
                        )
                        .await;
                }
            }
        };

        // Pre-gather phase done — `/gather extend` is rejected from here on.
        *state.pregather.lock().unwrap() = None;
        drop(stream);

        if pregather_cancelled {
            let _ = msg
                .edit(
                    &serenity_ctx.http,
                    EditMessage::new()
                        .embeds(pregather_message_embeds(
                            serenity_ctx,
                            guild_id,
                            voice_channel_id,
                            &state,
                            pregather_started_at,
                            pregather_started_at_wall,
                            pregather_duration,
                            &author_mention,
                            &schedule_label,
                            Some("Cancelled."),
                        ))
                        .components(Vec::new()),
                )
                .await;
            return Ok(());
        }
    } else {
        // No countdown (auto-gather after a break): ping voice members in a
        // separate message above, then seed a placeholder Phase 2 edits into
        // the check-in embed.
        let voice_mentions: String = initial_voice_ids
            .iter()
            .map(|id| id.mention().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        if !voice_mentions.is_empty() {
            let _ = text_channel_id
                .send_message(
                    &serenity_ctx.http,
                    CreateMessage::new().content(voice_mentions),
                )
                .await;
        }
        msg = text_channel_id
            .send_message(
                &serenity_ctx.http,
                CreateMessage::new().content("Gathering starting…"),
            )
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
    }

    // ── Phase 2: gathering check-in. Re-read voice members because people
    // may have joined during the countdown.
    let mut expected: HashSet<UserId> = current_voice_members(serenity_ctx, guild_id, voice_channel_id, bot_id)
        .into_iter()
        .collect();
    expected.insert(author_id);
    {
        let extra = state.extra_expected.lock().unwrap();
        for id in extra.iter() {
            expected.insert(*id);
        }
    }

    let started_at = Instant::now();
    let mut grace_ends_at = started_at + GRACE_PERIOD;
    let deadline = started_at + MAX_GATHER_DURATION;

    let mut arrivals: HashMap<UserId, Duration> = HashMap::new();
    let mut opted_out: HashSet<UserId> = HashSet::new();

    let silent = *state.silent.lock().unwrap();
    let _ = msg
        .edit(
            &serenity_ctx.http,
            EditMessage::new()
                .content("")
                .embed(check_in_embed(
                    serenity_ctx,
                    guild_id,
                    &expected,
                    &arrivals,
                    &opted_out,
                    started_at,
                    grace_ends_at,
                    silent,
                    None,
                ))
                .components(gather_buttons(false, silent)),
        )
        .await;

    let mut last_ghost_ping = started_at;
    let mut last_edit = Instant::now();
    let mut cancelled = false;

    // All clicks are deferred on arrival in a spawned task — keeps the 3s
    // ack window safe even when this loop is busy rendering or talking to
    // Discord. Interactions arrive here already acked.
    let mut stream = DeferredInteractionStream::new(serenity_ctx, msg.id);

    loop {
        let now = Instant::now();
        if now >= deadline || cancelled {
            break;
        }

        if expected.iter().all(|id| arrivals.contains_key(id)) {
            grace_ends_at = grace_ends_at.min(now);
            break;
        }

        let next_periodic = if now < grace_ends_at {
            grace_ends_at
        } else {
            (last_ghost_ping + GHOST_PING_INTERVAL).min(deadline)
        };
        let wait = next_periodic.saturating_duration_since(now).min(LOOP_POLL);

        if let Some(ic) = stream.next_within(wait).await {
            let outcome = handle_interaction(
                &ic,
                serenity_ctx,
                guild_id,
                voice_channel_id,
                author_id,
                started_at,
                &mut grace_ends_at,
                &mut expected,
                &mut arrivals,
                &mut opted_out,
                &state,
            )
            .await;

            match outcome {
                CheckInOutcome::Cancel => cancelled = true,
                CheckInOutcome::Refresh => last_edit = started_at, // force immediate refresh
                CheckInOutcome::None => {}
            }
        }

        let now = Instant::now();

        {
            let extra = state.extra_expected.lock().unwrap();
            for id in extra.iter() {
                expected.insert(*id);
            }
        }

        // `/gather forget` queues drops here — applied unless the user has already arrived.
        {
            let forgotten: Vec<UserId> = state.forgotten.lock().unwrap().drain().collect();
            for id in forgotten {
                if !arrivals.contains_key(&id) {
                    expected.remove(&id);
                }
            }
        }

        // voice_handler reports joins by inserting into auto_arrived.
        {
            let auto_ids: Vec<UserId> = state.auto_arrived.lock().unwrap().drain().collect();
            if !auto_ids.is_empty() {
                for id in auto_ids {
                    if expected.contains(&id) && !arrivals.contains_key(&id) {
                        let lateness = if now <= grace_ends_at { Duration::ZERO } else { now - started_at };
                        arrivals.insert(id, lateness);
                        if expected.iter().all(|id2| arrivals.contains_key(id2)) {
                            grace_ends_at = grace_ends_at.min(now);
                        }
                    }
                }
                last_edit = started_at; // force embed refresh on next throttle check
            }
        }

        let silent = *state.silent.lock().unwrap();

        // Ghost-ping missing members after grace expires (unless silenced).
        if !silent && now >= grace_ends_at && now >= last_ghost_ping + GHOST_PING_INTERVAL {
            last_ghost_ping = now;
            let missing: Vec<UserId> = expected
                .iter()
                .filter(|id| !arrivals.contains_key(id))
                .copied()
                .collect();
            if !missing.is_empty() {
                tokio::spawn(ghost_ping(
                    serenity_ctx.http.clone(),
                    text_channel_id,
                    missing,
                ));
            }
        }

        if Instant::now() >= last_edit + MIN_EDIT_INTERVAL {
            last_edit = Instant::now();
            let _ = msg
                .edit(
                    &serenity_ctx.http,
                    EditMessage::new()
                        .embed(check_in_embed(
                            serenity_ctx,
                            guild_id,
                            &expected,
                            &arrivals,
                            &opted_out,
                            started_at,
                            grace_ends_at,
                            silent,
                            None,
                        ))
                        .components(gather_buttons(false, silent)),
                )
                .await;
        }
    }

    drop(stream);

    let silent = *state.silent.lock().unwrap();
    let footer = if cancelled {
        Some("Cancelled by initiator.")
    } else if Instant::now() >= deadline {
        Some("Gathering timed out.")
    } else {
        Some("All checked in. Gathering complete.")
    };

    let _ = msg
        .edit(
            &serenity_ctx.http,
            EditMessage::new()
                .embed(check_in_embed(
                    serenity_ctx,
                    guild_id,
                    &expected,
                    &arrivals,
                    &opted_out,
                    started_at,
                    grace_ends_at,
                    silent,
                    footer,
                ))
                .components(Vec::new()),
        )
        .await;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_interaction(
    ic: &ComponentInteraction,
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    author_id: UserId,
    started_at: Instant,
    grace_ends_at: &mut Instant,
    expected: &mut HashSet<UserId>,
    arrivals: &mut HashMap<UserId, Duration>,
    opted_out: &mut HashSet<UserId>,
    state: &GatherState,
) -> CheckInOutcome {
    match ic.data.custom_id.as_str() {
        BTN_CANCEL => {
            if ic.user.id != author_id {
                send_ephemeral(
                    &serenity_ctx.http,
                    ic,
                    "Only the person who started the gathering can cancel it.",
                )
                .await;
                return CheckInOutcome::None;
            }
            CheckInOutcome::Cancel
        }
        BTN_NOT_COMING => {
            if arrivals.contains_key(&ic.user.id) {
                send_ephemeral(
                    &serenity_ctx.http,
                    ic,
                    "You've already checked in — you can't opt out now.",
                )
                .await;
                return CheckInOutcome::None;
            }
            if opted_out.contains(&ic.user.id) {
                send_ephemeral(
                    &serenity_ctx.http,
                    ic,
                    "You've already marked yourself as not coming.",
                )
                .await;
                return CheckInOutcome::None;
            }

            expected.remove(&ic.user.id);
            opted_out.insert(ic.user.id);

            let now = Instant::now();
            if expected.iter().all(|id| arrivals.contains_key(id)) {
                *grace_ends_at = now.min(*grace_ends_at);
            }

            CheckInOutcome::Refresh
        }
        BTN_TOGGLE_SILENT => {
            if ic.user.id != author_id {
                send_ephemeral(
                    &serenity_ctx.http,
                    ic,
                    "Only the person who started the gathering can mute pings.",
                )
                .await;
                return CheckInOutcome::None;
            }
            {
                let mut s = state.silent.lock().unwrap();
                *s = !*s;
            }
            CheckInOutcome::Refresh
        }
        BTN_HERE => {
            if !user_in_voice(serenity_ctx, guild_id, voice_channel_id, ic.user.id) {
                send_ephemeral(
                    &serenity_ctx.http,
                    ic,
                    "You need to be in the voice channel to check in.",
                )
                .await;
                return CheckInOutcome::None;
            }
            if arrivals.contains_key(&ic.user.id) {
                send_ephemeral(&serenity_ctx.http, ic, "You're already checked in.").await;
                return CheckInOutcome::None;
            }

            let now = Instant::now();
            let lateness = if now <= *grace_ends_at { Duration::ZERO } else { now - started_at };

            arrivals.insert(ic.user.id, lateness);
            expected.insert(ic.user.id);

            if expected.iter().all(|id| arrivals.contains_key(id)) {
                *grace_ends_at = now;
            }

            CheckInOutcome::Refresh
        }
        _ => CheckInOutcome::None,
    }
}

async fn send_ephemeral(
    http: &Http,
    ic: &ComponentInteraction,
    content: &str,
) {
    let _ = ic
        .create_followup(
            http,
            CreateInteractionResponseFollowup::new()
                .content(content)
                .ephemeral(true),
        )
        .await;
}

fn check_in_embed(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    expected: &HashSet<UserId>,
    arrivals: &HashMap<UserId, Duration>,
    opted_out: &HashSet<UserId>,
    started_at: Instant,
    grace_ends_at: Instant,
    silent: bool,
    footer: Option<&str>,
) -> serenity::all::CreateEmbed {
    let rows: Vec<CheckInRow> = {
        let guild = serenity_ctx.cache.guild(guild_id);
        let resolve_name = |id: &UserId| {
            guild
                .as_ref()
                .and_then(|g| g.members.get(id))
                .map(|m| m.display_name().to_string())
                .unwrap_or_else(|| format!("User {}", id.get()))
        };

        let mut rows: Vec<CheckInRow> = expected
            .iter()
            .map(|id| CheckInRow {
                display_name: sanitize_name(&resolve_name(id)),
                arrived: arrivals.get(id).copied(),
                opted_out: false,
            })
            .collect();

        for id in opted_out {
            rows.push(CheckInRow {
                display_name: sanitize_name(&resolve_name(id)),
                arrived: None,
                opted_out: true,
            });
        }

        rows
    };

    GatherEmbed::CheckIn {
        rows: &rows,
        started_at,
        grace_ends_at,
        silent,
        footer,
    }
    .to_embed()
}

fn current_voice_members(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    bot_id: UserId,
) -> Vec<UserId> {
    serenity_ctx
        .cache
        .guild(guild_id)
        .as_ref()
        .map(|g| {
            g.voice_states
                .values()
                .filter(|vs| vs.channel_id == Some(voice_channel_id) && vs.user_id != bot_id)
                .map(|vs| vs.user_id)
                .collect()
        })
        .unwrap_or_default()
}

fn user_in_voice(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    user_id: UserId,
) -> bool {
    serenity_ctx
        .cache
        .guild(guild_id)
        .as_ref()
        .and_then(|g| g.voice_states.get(&user_id))
        .and_then(|vs| vs.channel_id)
        == Some(voice_channel_id)
}

/// Returns the embed pair posted on the gathering message during the
/// scheduled pre-gather countdown: the live attendee list on top, the
/// countdown embed below. Order matches `/break` for visual consistency.
#[allow(clippy::too_many_arguments)]
fn pregather_message_embeds(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    state: &GatherState,
    pregather_started_at: Instant,
    pregather_started_at_wall: OffsetDateTime,
    original_duration: Duration,
    author_mention: &str,
    schedule_label: &str,
    footer: Option<&str>,
) -> Vec<CreateEmbed> {
    let extra = state.extra_expected.lock().unwrap().clone();
    let forgotten = state.forgotten.lock().unwrap().clone();
    let attendees = attendance_service::attendees_embed(serenity_ctx, guild_id, voice_channel_id, &extra, &forgotten);
    let countdown = pregather_embed(
        state,
        pregather_started_at,
        pregather_started_at_wall,
        original_duration,
        author_mention,
        schedule_label,
        footer,
    );
    vec![attendees, countdown]
}

#[allow(clippy::too_many_arguments)]
fn pregather_embed(
    state: &GatherState,
    pregather_started_at: Instant,
    pregather_started_at_wall: OffsetDateTime,
    original_duration: Duration,
    author_mention: &str,
    schedule_label: &str,
    footer: Option<&str>,
) -> serenity::all::CreateEmbed {
    let extension = *state.pregather_extension.lock().unwrap();
    let total = original_duration + extension;
    GatherEmbed::Pregather {
        ends_at: pregather_started_at + total,
        ends_at_wall: pregather_started_at_wall + total,
        author_mention,
        schedule_label,
        extension,
        original_duration,
        footer,
    }
    .to_embed()
}

async fn ghost_ping(
    http: Arc<Http>,
    text_channel_id: ChannelId,
    users: Vec<UserId>,
) {
    let content = users
        .iter()
        .map(|u| u.mention().to_string())
        .collect::<Vec<_>>()
        .join(" ");

    let sent = text_channel_id
        .send_message(&http, CreateMessage::new().content(content))
        .await;

    if let Ok(m) = sent {
        let http_clone = http.clone();
        let ch = text_channel_id;
        let mid = m.id;
        tokio::spawn(async move {
            tokio::time::sleep(GHOST_PING_LIFETIME).await;
            let _ = http_clone
                .delete_message(ch, mid, Some("gather ghost ping"))
                .await;
        });
    }
}
