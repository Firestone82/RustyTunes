use crate::bot::MusicBotError;
use serenity::all::{
    ButtonStyle, ChannelId, Color, ComponentInteractionCollector, CreateActionRow, CreateButton,
    CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
    EditMessage, GuildId, Mentionable, Message, UserId,
};
use serenity::futures::StreamExt;
use serenity::http::Http;
use serenity::prelude::Context as SerenityContext;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const GRACE_PERIOD: Duration = Duration::from_secs(60);
const GHOST_PING_INTERVAL: Duration = Duration::from_secs(30);
const MAX_GATHER_DURATION: Duration = Duration::from_secs(60 * 30);
const GHOST_PING_LIFETIME: Duration = Duration::from_millis(700);
const MIN_EDIT_INTERVAL: Duration = Duration::from_secs(5);
const MAX_NAME_LEN: usize = 22;
// Max wait in the select loop — keeps button response latency well within 3 s.
const LOOP_POLL: Duration = Duration::from_millis(800);

const BTN_HERE: &str = "gather_im_here";
const BTN_CANCEL: &str = "gather_cancel";
const BTN_FORCE_START: &str = "gather_force_start";

pub async fn start_gather(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    text_channel_id: ChannelId,
    voice_channel_id: ChannelId,
    author_id: UserId,
) -> Result<(), MusicBotError> {
    let bot_id = serenity_ctx.cache.current_user().id;

    let expected_ids: Vec<UserId> =
        current_voice_members(serenity_ctx, guild_id, voice_channel_id, bot_id);

    if expected_ids.is_empty() {
        return Err(MusicBotError::InternalError(
            "No one is in the voice channel.".to_string(),
        ));
    }

    let started_at = Instant::now();
    let mut grace_ends_at = started_at + GRACE_PERIOD;
    let deadline = started_at + MAX_GATHER_DURATION;

    let mut arrivals: HashMap<UserId, Duration> = HashMap::new();
    let mut expected: HashSet<UserId> = expected_ids.iter().copied().collect();
    expected.insert(author_id);

    // Ping only the users currently in voice instead of @here.
    let voice_mentions: String = expected_ids
        .iter()
        .map(|id| id.mention().to_string())
        .collect::<Vec<_>>()
        .join(" ");

    let mut msg: Message = text_channel_id
        .send_message(
            &serenity_ctx.http,
            CreateMessage::new()
                .content(format!(
                    "{voice_mentions}  📣  Gathering in voice channel — click **I'm here!** below."
                ))
                .embed(build_embed(
                    serenity_ctx,
                    guild_id,
                    &expected,
                    &arrivals,
                    started_at,
                    grace_ends_at,
                    None,
                ))
                .components(buttons(false)),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

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

        // If everyone has arrived, end the grace period right now and exit.
        if expected.iter().all(|id| arrivals.contains_key(id)) {
            grace_ends_at = grace_ends_at.min(now);
            break;
        }

        let next_periodic = if now < grace_ends_at {
            grace_ends_at
        } else {
            (last_ghost_ping + GHOST_PING_INTERVAL).min(deadline)
        };
        let wait = next_periodic
            .saturating_duration_since(now)
            .min(LOOP_POLL);

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
                    )
                    .await,
                    None => break,
                }
            }
            _ = tokio::time::sleep(wait) => {}
        }

        let now = Instant::now();

        // Track late joiners from voice channel.
        for id in current_voice_members(serenity_ctx, guild_id, voice_channel_id, bot_id) {
            expected.insert(id);
        }

        // Ghost-ping missing members after grace expires.
        if now >= grace_ends_at && now >= last_ghost_ping + GHOST_PING_INTERVAL {
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

        // Throttled periodic embed refresh.
        if Instant::now() >= last_edit + MIN_EDIT_INTERVAL {
            last_edit = Instant::now();
            let _ = msg
                .edit(
                    &serenity_ctx.http,
                    EditMessage::new()
                        .embed(build_embed(
                            serenity_ctx,
                            guild_id,
                            &expected,
                            &arrivals,
                            started_at,
                            grace_ends_at,
                            None,
                        ))
                        .components(buttons(false)),
                )
                .await;
        }
    }

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
                .embed(build_embed(
                    serenity_ctx,
                    guild_id,
                    &expected,
                    &arrivals,
                    started_at,
                    grace_ends_at,
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
                            .content(
                                "Only the person who started the gathering can force-start it.",
                            )
                            .ephemeral(true),
                    ),
                )
                .await
                .ok();
                return;
            }
            *grace_ends_at = Instant::now();
            ic.create_response(
                &serenity_ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .embed(build_embed(
                            serenity_ctx,
                            guild_id,
                            expected,
                            arrivals,
                            started_at,
                            *grace_ends_at,
                            None,
                        ))
                        .components(buttons(false)),
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
            let lateness = if now <= *grace_ends_at {
                Duration::ZERO
            } else {
                now - started_at
            };

            arrivals.insert(ic.user.id, lateness);
            expected.insert(ic.user.id);

            // If everyone has now arrived during grace, end grace immediately.
            if expected.iter().all(|id| arrivals.contains_key(id)) {
                *grace_ends_at = now;
            }

            ic.create_response(
                &serenity_ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .embed(build_embed(
                            serenity_ctx,
                            guild_id,
                            expected,
                            arrivals,
                            started_at,
                            *grace_ends_at,
                            None,
                        ))
                        .components(buttons(false)),
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

fn buttons(disabled: bool) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(BTN_HERE)
            .label("I'm here!")
            .style(ButtonStyle::Success)
            .disabled(disabled),
        CreateButton::new(BTN_FORCE_START)
            .label("Force start")
            .style(ButtonStyle::Primary)
            .disabled(disabled),
        CreateButton::new(BTN_CANCEL)
            .label("Cancel")
            .style(ButtonStyle::Danger)
            .disabled(disabled),
    ])]
}

fn build_embed(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    expected: &HashSet<UserId>,
    arrivals: &HashMap<UserId, Duration>,
    started_at: Instant,
    grace_ends_at: Instant,
    footer: Option<&str>,
) -> CreateEmbed {
    let now = Instant::now();
    let in_grace = now < grace_ends_at;
    let grace_remaining = grace_ends_at.saturating_duration_since(now);

    let names: HashMap<UserId, String> = {
        let guild = serenity_ctx.cache.guild(guild_id);
        expected
            .iter()
            .map(|id| {
                let raw = guild
                    .as_ref()
                    .and_then(|g| g.members.get(id))
                    .map(|m| m.display_name().to_string())
                    .unwrap_or_else(|| format!("User {}", id.get()));
                (*id, sanitize_name(&raw))
            })
            .collect()
    };

    let mut rows: Vec<(String, String)> = expected
        .iter()
        .map(|id| {
            let name = names.get(id).cloned().unwrap_or_default();
            let status = match arrivals.get(id) {
                Some(d) if d.is_zero() => "ON TIME".to_string(),
                Some(d) => format!("+{}", format_mmss(*d)),
                None => "--:--".to_string(),
            };
            (name, status)
        })
        .collect();

    rows.sort_by(|a, b| {
        let aa = arrivals_order(arrivals, a, &names);
        let bb = arrivals_order(arrivals, b, &names);
        aa.cmp(&bb)
    });

    let name_width = rows
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, MAX_NAME_LEN);
    let status_width = rows
        .iter()
        .map(|(_, s)| s.chars().count())
        .max()
        .unwrap_or(7)
        .max(7);

    let mut table = String::new();
    let sep = format!(
        "+{}+{}+\n",
        "-".repeat(name_width + 2),
        "-".repeat(status_width + 2)
    );
    table.push_str(&sep);
    table.push_str(&format!(
        "| {:<nw$} | {:<sw$} |\n",
        "User",
        "Arrived",
        nw = name_width,
        sw = status_width
    ));
    table.push_str(&sep);
    for (name, status) in &rows {
        let trimmed: String = name.chars().take(name_width).collect();
        table.push_str(&format!(
            "| {:<nw$} | {:<sw$} |\n",
            trimmed,
            status,
            nw = name_width,
            sw = status_width
        ));
    }
    table.push_str(&sep);

    let elapsed = now.saturating_duration_since(started_at);
    let header = if in_grace {
        format!(
            "Grace period: **{}** remaining (counting starts at {}).",
            format_mmss(grace_remaining),
            format_mmss(GRACE_PERIOD)
        )
    } else {
        format!(
            "Counting since gather started — elapsed: **{}**.\nLate arrivals are stamped with their time-from-start.",
            format_mmss(elapsed)
        )
    };

    let present = arrivals.len();
    let total = expected.len();

    let color = if footer.is_some() {
        Color::DARK_GREEN
    } else if in_grace {
        Color::DARK_BLUE
    } else {
        Color::ORANGE
    };

    let mut builder = CreateEmbed::new()
        .color(color)
        .title("📣  Voice Channel Gathering")
        .description(format!(
            "{}\n\nAttendance: **{}/{}**\n```\n{}```",
            header, present, total, table
        ));

    if let Some(text) = footer {
        builder = builder.footer(serenity::all::CreateEmbedFooter::new(text));
    }

    builder
}

fn arrivals_order(
    arrivals: &HashMap<UserId, Duration>,
    row: &(String, String),
    names: &HashMap<UserId, String>,
) -> (u8, u128, String) {
    let id_for_name = names
        .iter()
        .find_map(|(id, n)| if n == &row.0 { Some(*id) } else { None });
    match id_for_name.and_then(|id| arrivals.get(&id)) {
        Some(d) => (0, d.as_millis(), row.0.clone()),
        None => (1, 0, row.0.clone()),
    }
}

fn format_mmss(d: Duration) -> String {
    let total = d.as_secs();
    let m = total / 60;
    let s = total % 60;
    format!("{:02}:{:02}", m, s)
}

/// Replace emoji grapheme clusters with their `:shortcode:` then truncate
/// to MAX_NAME_LEN so the table stays aligned in Discord's monospace font.
fn sanitize_name(name: &str) -> String {
    use unicode_segmentation::UnicodeSegmentation;

    let mut out = String::new();
    for g in name.graphemes(true) {
        if let Some(emoji) = emojis::get(g) {
            let label = emoji.shortcode().unwrap_or(emoji.name());
            out.push(':');
            out.push_str(label.trim_matches(':'));
            out.push(':');
        } else {
            out.push_str(g);
        }
    }

    out.chars().take(MAX_NAME_LEN).collect()
}

async fn ghost_ping(http: Arc<Http>, text_channel_id: ChannelId, users: Vec<UserId>) {
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
