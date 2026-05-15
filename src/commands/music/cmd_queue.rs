use crate::bot::{Context, MusicBotError};
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::embeds::music::queue_embed::QueueEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, EditMessage};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLockReadGuard;

const ITEMS_PER_PAGE: usize = 10;

fn nav_buttons(page: usize, total_pages: usize) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new("queue_prev")
            .label("◀")
            .style(ButtonStyle::Secondary)
            .disabled(page <= 1),
        CreateButton::new("queue_next")
            .label("▶")
            .style(ButtonStyle::Secondary)
            .disabled(page >= total_pages),
    ])]
}

/// Build the embed list rendered for `!queue`: a Now Playing embed (when a
/// track is active) followed by the standard queue listing. Returns an empty
/// vec when there is nothing to show — callers fall back to `IsEmpty`.
fn build_embeds(player: &Player, page: usize) -> Vec<CreateEmbed> {
    let mut embeds: Vec<CreateEmbed> = Vec::new();

    if let Some(track) = player.current_track.as_ref() {
        embeds.push(PlayerEmbed::NowPlaying(track).to_embed());
    }

    if !player.queue.is_empty() {
        embeds.push(QueueEmbed::Current { queue: &player.queue, page }.to_embed());
    }

    embeds
}

/// List upcoming tracks in the queue.
#[poise::command(prefix_command, slash_command)]
pub async fn queue(ctx: Context<'_>, page: Option<usize>) -> Result<(), MusicBotError> {
    let player: RwLockReadGuard<Player> = ctx.data().player.read().await;

    if player.queue.is_empty() && player.current_track.is_none() {
        drop(player);
        QueueEmbed::IsEmpty
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    let total_pages = player.queue.len().div_ceil(ITEMS_PER_PAGE).max(1);
    let mut page = page.unwrap_or(1).max(1).min(total_pages);

    let embeds = build_embeds(&player, page);
    let needs_pagination = total_pages > 1 && !player.queue.is_empty();
    drop(player);

    // Single message with both embeds — Now Playing then queue list.
    let mut reply = poise::CreateReply::default().reply(true);
    for embed in embeds {
        reply = reply.embed(embed);
    }

    if !needs_pagination {
        ctx.send(reply)
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
        return Ok(());
    }

    let reply_handle = ctx
        .send(reply.components(nav_buttons(page, total_pages)))
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let mut message = reply_handle
        .into_message()
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let http = ctx.serenity_context().http.clone();
    let mut cooldowns: HashMap<serenity::all::UserId, Instant> = HashMap::new();

    loop {
        let interaction = message
            .await_component_interaction(ctx.serenity_context().shard.clone())
            .timeout(Duration::from_secs(60))
            .await;

        match interaction {
            Some(interaction) => {
                if interaction.user.id != ctx.author().id {
                    let now = Instant::now();
                    let on_cooldown = cooldowns
                        .get(&interaction.user.id)
                        .map(|&last| now.duration_since(last) < Duration::from_secs(5))
                        .unwrap_or(false);
                    if on_cooldown {
                        interaction.defer(&http).await.ok();
                    } else {
                        cooldowns.insert(interaction.user.id, now);
                        interaction
                            .create_response(
                                &http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Only the person who ran this command can navigate the queue.")
                                        .ephemeral(true),
                                ),
                            )
                            .await
                            .ok();
                    }
                    continue;
                }

                interaction
                    .defer(&http)
                    .await
                    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

                let player = ctx.data().player.read().await;

                if player.queue.is_empty() && player.current_track.is_none() {
                    drop(player);
                    let _ = message
                        .edit(
                            &http,
                            EditMessage::new()
                                .embeds(vec![QueueEmbed::IsEmpty.to_embed()])
                                .components(vec![]),
                        )
                        .await;
                    break;
                }

                let total_pages = player.queue.len().div_ceil(ITEMS_PER_PAGE).max(1);

                match interaction.data.custom_id.as_str() {
                    "queue_prev" => page = page.saturating_sub(1).max(1),
                    "queue_next" => page = (page + 1).min(total_pages),
                    _ => {}
                }
                page = page.min(total_pages);

                let embeds = build_embeds(&player, page);
                let still_paginates = total_pages > 1 && !player.queue.is_empty();
                drop(player);

                let mut edit = EditMessage::new().embeds(embeds);
                edit = if still_paginates {
                    edit.components(nav_buttons(page, total_pages))
                } else {
                    edit.components(vec![])
                };
                let _ = message.edit(&http, edit).await;
            }
            None => {
                let _ = message.delete(&http).await;
                break;
            }
        }
    }

    Ok(())
}
