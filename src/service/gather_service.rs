use crate::bot::MusicBotError;
use crate::embeds::activity::gather_embed::{gather_buttons, pregather_buttons, CheckInRow, GatherEmbed, BTN_CANCEL, BTN_FORCE_START, BTN_HERE, BTN_TOGGLE_SILENT, GRACE_PERIOD};
use crate::service::attendance_service;
use crate::utils::string_utils::sanitize_name;
use crate::utils::time_utils::get_current_time;
use serenity::all::{
    ChannelId, ComponentInteractionCollector, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditMessage, GuildId, Mentionable, Message, UserId,
};
use serenity::futures::StreamExt;
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
    let shard = serenity_ctx.shard.clone();

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
        // Voice members are pinged in the embed message's content field so the
        // @mentions stay glued to the embed (separate messages tended to get
        // visually orphaned as the embed refreshed).
        let voice_mentions: String = initial_voice_ids
            .iter()
            .map(|id| id.mention().to_string())
            .collect::<Vec<_>>()
            .join(" ");

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
                    .content(voice_mentions)
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

        let pregather_cancelled = 'pregather: loop {
            let now = Instant::now();
            let ends_at = pregather_started_at + pregather_duration + *state.pregather_extension.lock().unwrap();
            if now >= ends_at {
                break false;
            }

            let wait = ends_at
                .saturating_duration_since(now)
                .min(MIN_EDIT_INTERVAL);

            match msg
                .await_component_interaction(shard.clone())
                .timeout(wait)
                .await
            {
                Some(ic) => match ic.data.custom_id.as_str() {
                    BTN_CANCEL => {
                        if ic.user.id != author_id {
                            ic.create_response(
                                &serenity_ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Only the person who started the gathering can cancel it.")
                                        .ephemeral(true),
                                ),
                            )
                            .await
                            .ok();
                            continue 'pregather;
                        }
                        ic.create_response(&serenity_ctx.http, CreateInteractionResponse::Acknowledge)
                            .await
                            .ok();
                        break 'pregather true;
                    }
                    BTN_FORCE_START => {
                        if ic.user.id != author_id {
                            ic.create_response(
                                &serenity_ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Only the person who started the gathering can skip the countdown.")
                                        .ephemeral(true),
                                ),
                            )
                            .await
                            .ok();
                            continue 'pregather;
                        }
                        ic.create_response(&serenity_ctx.http, CreateInteractionResponse::Acknowledge)
                            .await
                            .ok();
                        break 'pregather false;
                    }
                    _ => {
                        ic.create_response(&serenity_ctx.http, CreateInteractionResponse::Acknowledge)
                            .await
                            .ok();
                    }
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
        // No countdown: ping voice members and seed a message Phase 2 will
        // edit into the check-in embed.
        let voice_mentions: String = initial_voice_ids
            .iter()
            .map(|id| id.mention().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        msg = text_channel_id
            .send_message(
                &serenity_ctx.http,
                CreateMessage::new().content(if voice_mentions.is_empty() { "Gathering starting…".to_string() } else { voice_mentions }),
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

    let silent = *state.silent.lock().unwrap();
    let _ = msg
        .edit(
            &serenity_ctx.http,
            EditMessage::new()
                // Leave `.content` untouched — the @mentions seeded on the
                // initial send fire the ping once and remain visible above
                // the embed for the rest of the gathering.
                .embeds(check_in_message_embeds(
                    serenity_ctx,
                    guild_id,
                    voice_channel_id,
                    &state,
                    &expected,
                    &arrivals,
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

    // A persistent stream buffers every interaction on this message so no
    // button click is ever dropped between loop iterations.
    let interaction_stream = ComponentInteractionCollector::new(serenity_ctx)
        .message_id(msg.id)
        .stream();
    tokio::pin!(interaction_stream);

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

        tokio::select! {
            ic = interaction_stream.next() => {
                match ic {
                    Some(ic) => handle_interaction(
                        &ic,
                        serenity_ctx,
                        guild_id,
                        voice_channel_id,
                        author_id,
                        started_at,
                        &mut grace_ends_at,
                        &mut expected,
                        &mut arrivals,
                        &mut cancelled,
                        &mut last_edit,
                        &state,
                    )
                    .await,
                    None => break,
                }
            }
            _ = tokio::time::sleep(wait) => {}
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
                        .embeds(check_in_message_embeds(
                            serenity_ctx,
                            guild_id,
                            voice_channel_id,
                            &state,
                            &expected,
                            &arrivals,
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
                .embeds(check_in_message_embeds(
                    serenity_ctx,
                    guild_id,
                    voice_channel_id,
                    &state,
                    &expected,
                    &arrivals,
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
    ic: &serenity::all::ComponentInteraction,
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    author_id: UserId,
    started_at: Instant,
    grace_ends_at: &mut Instant,
    expected: &mut HashSet<UserId>,
    arrivals: &mut HashMap<UserId, Duration>,
    cancelled: &mut bool,
    last_edit: &mut Instant,
    state: &GatherState,
) {
    match ic.data.custom_id.as_str() {
        BTN_CANCEL => {
            if ic.user.id != author_id {
                ic.create_response(
                    &serenity_ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Only the person who started the gathering can cancel it.")
                            .ephemeral(true),
                    ),
                )
                .await
                .ok();
                return;
            }
            ic.create_response(&serenity_ctx.http, CreateInteractionResponse::Acknowledge)
                .await
                .ok();
            *cancelled = true;
        }
        BTN_FORCE_START => {
            if ic.user.id != author_id {
                ic.create_response(
                    &serenity_ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Only the person who started the gathering can force-start it.")
                            .ephemeral(true),
                    ),
                )
                .await
                .ok();
                return;
            }
            *grace_ends_at = Instant::now();
            let silent = *state.silent.lock().unwrap();
            ic.create_response(
                &serenity_ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .embeds(check_in_message_embeds(
                            serenity_ctx,
                            guild_id,
                            voice_channel_id,
                            state,
                            expected,
                            arrivals,
                            started_at,
                            *grace_ends_at,
                            silent,
                            None,
                        ))
                        .components(gather_buttons(false, silent)),
                ),
            )
            .await
            .ok();
            *last_edit = Instant::now();
        }
        BTN_TOGGLE_SILENT => {
            if ic.user.id != author_id {
                ic.create_response(
                    &serenity_ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Only the person who started the gathering can mute pings.")
                            .ephemeral(true),
                    ),
                )
                .await
                .ok();
                return;
            }
            let new_silent = {
                let mut s = state.silent.lock().unwrap();
                *s = !*s;
                *s
            };
            ic.create_response(
                &serenity_ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .embeds(check_in_message_embeds(
                            serenity_ctx,
                            guild_id,
                            voice_channel_id,
                            state,
                            expected,
                            arrivals,
                            started_at,
                            *grace_ends_at,
                            new_silent,
                            None,
                        ))
                        .components(gather_buttons(false, new_silent)),
                ),
            )
            .await
            .ok();
            *last_edit = Instant::now();
        }
        BTN_HERE => {
            if !user_in_voice(serenity_ctx, guild_id, voice_channel_id, ic.user.id) {
                ic.create_response(
                    &serenity_ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("You need to be in the voice channel to check in.")
                            .ephemeral(true),
                    ),
                )
                .await
                .ok();
                return;
            }

            if arrivals.contains_key(&ic.user.id) {
                ic.create_response(
                    &serenity_ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("You're already checked in.")
                            .ephemeral(true),
                    ),
                )
                .await
                .ok();
                return;
            }

            let now = Instant::now();
            let lateness = if now <= *grace_ends_at { Duration::ZERO } else { now - started_at };

            arrivals.insert(ic.user.id, lateness);
            expected.insert(ic.user.id);

            if expected.iter().all(|id| arrivals.contains_key(id)) {
                *grace_ends_at = now;
            }

            let silent = *state.silent.lock().unwrap();
            ic.create_response(
                &serenity_ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .embeds(check_in_message_embeds(
                            serenity_ctx,
                            guild_id,
                            voice_channel_id,
                            state,
                            expected,
                            arrivals,
                            started_at,
                            *grace_ends_at,
                            silent,
                            None,
                        ))
                        .components(gather_buttons(false, silent)),
                ),
            )
            .await
            .ok();
            *last_edit = Instant::now();
        }
        _ => {
            ic.create_response(&serenity_ctx.http, CreateInteractionResponse::Acknowledge)
                .await
                .ok();
        }
    }
}

fn check_in_embed(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    expected: &HashSet<UserId>,
    arrivals: &HashMap<UserId, Duration>,
    started_at: Instant,
    grace_ends_at: Instant,
    silent: bool,
    footer: Option<&str>,
) -> serenity::all::CreateEmbed {
    let rows: Vec<CheckInRow> = {
        let guild = serenity_ctx.cache.guild(guild_id);
        expected
            .iter()
            .map(|id| {
                let raw = guild
                    .as_ref()
                    .and_then(|g| g.members.get(id))
                    .map(|m| m.display_name().to_string())
                    .unwrap_or_else(|| format!("User {}", id.get()));
                CheckInRow {
                    display_name: sanitize_name(&raw),
                    arrived: arrivals.get(id).copied(),
                }
            })
            .collect()
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
/// pre-gather countdown: the main countdown embed, followed by the live
/// attendee list (current voice channel members + `/expect`d users).
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
    let main = pregather_embed(
        state,
        pregather_started_at,
        pregather_started_at_wall,
        original_duration,
        author_mention,
        schedule_label,
        footer,
    );
    let attendees = state_attendees_embed(serenity_ctx, guild_id, voice_channel_id, state);
    vec![main, attendees]
}

/// Returns the embed pair posted on the gathering message during check-in:
/// the arrival table only. The check-in table already conveys attendance,
/// so the second attendees embed is intentionally omitted in this phase.
#[allow(clippy::too_many_arguments)]
fn check_in_message_embeds(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    _voice_channel_id: ChannelId,
    _state: &GatherState,
    expected: &HashSet<UserId>,
    arrivals: &HashMap<UserId, Duration>,
    started_at: Instant,
    grace_ends_at: Instant,
    silent: bool,
    footer: Option<&str>,
) -> Vec<CreateEmbed> {
    vec![check_in_embed(
        serenity_ctx,
        guild_id,
        expected,
        arrivals,
        started_at,
        grace_ends_at,
        silent,
        footer,
    )]
}

/// Build the attendees embed for the current state — a thin wrapper that
/// holds the locks on `extra_expected` and `forgotten` only long enough to
/// hand a snapshot to `attendance_service`.
fn state_attendees_embed(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    state: &GatherState,
) -> CreateEmbed {
    let extra = state.extra_expected.lock().unwrap().clone();
    let forgotten = state.forgotten.lock().unwrap().clone();
    attendance_service::attendees_embed(serenity_ctx, guild_id, voice_channel_id, &extra, &forgotten)
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
    let mentions = expected_mentions_text(state);
    GatherEmbed::Pregather {
        ends_at: pregather_started_at + total,
        ends_at_wall: pregather_started_at_wall + total,
        author_mention,
        schedule_label,
        extension,
        original_duration,
        expected_mentions: mentions.as_deref(),
        footer,
    }
    .to_embed()
}

/// Comma-separated mentions of the users `/expect` has queued, or `None` if
/// the list is empty.
pub fn expected_mentions_text(state: &GatherState) -> Option<String> {
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
