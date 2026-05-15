use crate::bot::MusicBotError;
use crate::embeds::activity::gather_embed::{gather_buttons, pregather_buttons, CheckInRow, GatherEmbed, BTN_CANCEL, BTN_FORCE_START, BTN_HERE, BTN_TOGGLE_SILENT, GRACE_PERIOD};
use crate::utils::string_utils::sanitize_name;
use crate::utils::time_utils::get_current_time;
use serenity::all::{ChannelId, ComponentInteractionCollector, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditMessage, GuildId, Mentionable, Message, UserId};
use serenity::futures::StreamExt;
use serenity::http::Http;
use serenity::prelude::Context as SerenityContext;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Shared state for an active gathering.
pub struct GatherState {
    /// The voice channel this gathering is tracking.
    pub voice_channel_id: ChannelId,
    /// Users added via `/gather expect` that must check in before gathering ends.
    pub extra_expected: Mutex<HashSet<UserId>>,
    /// Users who joined the gathering voice channel while expected — processed on the next loop tick.
    pub auto_arrived: Mutex<HashSet<UserId>>,
    /// When true, ghost-ping reminders are suppressed for everyone.
    pub silent: Mutex<bool>,
}

impl GatherState {
    pub fn new(voice_channel_id: ChannelId) -> Self {
        Self {
            voice_channel_id,
            extra_expected: Mutex::new(HashSet::new()),
            auto_arrived: Mutex::new(HashSet::new()),
            silent: Mutex::new(false),
        }
    }
}

pub const PREGATHER_DURATION: Duration = Duration::from_secs(60);
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
        // Ping voice members in a separate message above the embed.
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

        let pregather_ends_at = Instant::now() + pregather_duration;
        let pregather_ends_at_wall = get_current_time() + pregather_duration;

        msg = text_channel_id
            .send_message(
                &serenity_ctx.http,
                CreateMessage::new()
                    .embed(
                        GatherEmbed::Pregather {
                            ends_at: pregather_ends_at,
                            ends_at_wall: pregather_ends_at_wall,
                            author_mention: &author_mention,
                            schedule_label: &schedule_label,
                            footer: None,
                        }
                        .to_embed(),
                    )
                    .components(pregather_buttons(false)),
            )
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

        let pregather_cancelled = 'pregather: loop {
            let now = Instant::now();
            if now >= pregather_ends_at {
                break false;
            }

            let wait = pregather_ends_at
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
                    // Timeout: refresh the countdown display.
                    let _ = msg
                        .edit(
                            &serenity_ctx.http,
                            EditMessage::new()
                                .embed(
                                    GatherEmbed::Pregather {
                                        ends_at: pregather_ends_at,
                                        ends_at_wall: pregather_ends_at_wall,
                                        author_mention: &author_mention,
                                        schedule_label: &schedule_label,
                                        footer: None,
                                    }
                                    .to_embed(),
                                )
                                .components(pregather_buttons(false)),
                        )
                        .await;
                }
            }
        };

        if pregather_cancelled {
            let _ = msg
                .edit(
                    &serenity_ctx.http,
                    EditMessage::new()
                        .embed(
                            GatherEmbed::Pregather {
                                ends_at: pregather_ends_at,
                                ends_at_wall: pregather_ends_at_wall,
                                author_mention: &author_mention,
                                schedule_label: &schedule_label,
                                footer: Some("Cancelled."),
                            }
                            .to_embed(),
                        )
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
                .content("")
                .embed(check_in_embed(
                    serenity_ctx,
                    guild_id,
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
                .embed(check_in_embed(
                    serenity_ctx,
                    guild_id,
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
                        .embed(check_in_embed(
                            serenity_ctx,
                            guild_id,
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
                        .embed(check_in_embed(
                            serenity_ctx,
                            guild_id,
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
                        .embed(check_in_embed(
                            serenity_ctx,
                            guild_id,
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
