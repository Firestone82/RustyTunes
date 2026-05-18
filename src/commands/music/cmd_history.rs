use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::player::track::Track;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::interaction_service;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, Message};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use tokio::sync::RwLockWriteGuard;

/// Show the last 10 played tracks and optionally replay one.
#[poise::command(
    prefix_command,
    slash_command,
    check = "check_author_in_same_voice_channel"
)]
pub async fn history(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let player = ctx.data().player.read().await;
    let history: VecDeque<Track> = player.history.clone();
    drop(player);

    if history.is_empty() {
        return PlayerEmbed::HistoryEmpty
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await
            .map(|_| ());
    }

    // Build buttons in reverse order so button 1 = most recent
    let tracks_rev: Vec<Track> = history.iter().rev().cloned().collect();
    let buttons: Vec<CreateButton> = (0..tracks_rev.len())
        .map(|i| {
            CreateButton::new(format!("history_{}", i))
                .label((i + 1).to_string())
                .style(ButtonStyle::Secondary)
        })
        .collect();

    let row_count = buttons.len().div_ceil(5);
    let per_row = buttons.len().div_ceil(row_count.max(1));
    let rows: Vec<CreateActionRow> = buttons
        .chunks(per_row.max(1))
        .map(|chunk| CreateActionRow::Buttons(chunk.to_vec()))
        .collect();

    let reply_handle = ctx
        .send(
            poise::CreateReply::default()
                .embed(PlayerEmbed::History(&history).to_embed())
                .components(rows)
                .reply(true),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let message: Message = reply_handle
        .into_message()
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let deadline = Instant::now() + Duration::from_secs(60 * 2);
    let mut cooldowns: HashMap<serenity::all::UserId, Instant> = HashMap::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            message.delete(ctx.http()).await?;
            PlayerEmbed::SearchExpired
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(());
        }

        let interaction = message
            .await_component_interaction(ctx.serenity_context().shard.clone())
            .timeout(remaining)
            .await;

        match interaction {
            Some(interaction) => {
                // Ack first so the 3-second window can't race the gating
                // check or the join/queue work that follows.
                interaction_service::ack(&interaction, ctx.http()).await?;

                if interaction.user.id != ctx.author().id {
                    let now = Instant::now();
                    let on_cooldown = cooldowns
                        .get(&interaction.user.id)
                        .map(|&last| now.duration_since(last) < Duration::from_secs(5))
                        .unwrap_or(false);
                    if !on_cooldown {
                        cooldowns.insert(interaction.user.id, now);
                        interaction_service::reply_ephemeral(
                            &interaction,
                            ctx.http(),
                            "Only the person who ran this command can select a track.",
                        )
                        .await
                        .ok();
                    }
                    continue;
                }

                message.delete(ctx.http()).await?;

                let track_index: usize = interaction
                    .data
                    .custom_id
                    .strip_prefix("history_")
                    .and_then(|s| s.parse().ok())
                    .unwrap();
                let track: Track = tracks_rev[track_index].clone();

                let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
                player.add_track_to_queue(ctx, track, false).await?;
                drop(player);

                channel_service::join_user_channel(ctx).await?;
                break;
            }
            None => {
                message.delete(ctx.http()).await?;
                PlayerEmbed::SearchExpired
                    .to_embed()
                    .send_context(ctx, true, Some(15))
                    .await?;
                break;
            }
        }
    }

    Ok(())
}
