use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::checks::player_checks::check_if_queue_is_not_empty;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::{PlaybackError, Player};
use crate::service::embed_service::SendEmbed;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, EditMessage};
use std::time::Duration;
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

const ITEMS_PER_PAGE: usize = 10;

/// Show the queue. With no subcommand, lists upcoming tracks.
#[poise::command(
    prefix_command, slash_command,
    subcommands("list", "remove"),
)]
pub async fn queue(
    ctx: Context<'_>,
    #[description = "Page number"] page: Option<usize>,
) -> Result<(), MusicBotError> {
    list_inner(ctx, page).await
}

/// List upcoming tracks in the queue.
#[poise::command(prefix_command, slash_command)]
pub async fn list(
    ctx: Context<'_>,
    #[description = "Page number"] page: Option<usize>,
) -> Result<(), MusicBotError> {
    list_inner(ctx, page).await
}

/// Remove a track from the queue by 1-based index.
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_queue_is_not_empty",
)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Position of the track to remove (1 = first up next)"]
    index: usize,
) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    match player.remove_from_queue(index).await {
        Ok(track) => {
            drop(player);
            QueueEmbed::TrackRemoved(&track)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
        }
        Err(PlaybackError::InvalidQueueIndex(i)) => {
            drop(player);
            QueueEmbed::InvalidIndex(i)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

async fn list_inner(ctx: Context<'_>, page: Option<usize>) -> Result<(), MusicBotError> {
    let player: RwLockReadGuard<Player> = ctx.data().player.read().await;

    if player.queue.is_empty() {
        drop(player);
        QueueEmbed::IsEmpty
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    let total_pages = (player.queue.len() + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE;
    let mut page = page.unwrap_or(1).max(1).min(total_pages);

    // No pagination needed for a single page
    if total_pages <= 1 {
        QueueEmbed::Current { queue: &player.queue, page }
            .to_embed()
            .send_context(ctx, true, Some(60))
            .await?;
        return Ok(());
    }

    let embed = QueueEmbed::Current { queue: &player.queue, page }.to_embed();
    drop(player);

    let reply_handle = ctx.send(
        poise::CreateReply::default()
            .embed(embed)
            .components(nav_buttons(page, total_pages))
            .reply(true)
    ).await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let mut message = reply_handle.into_message().await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let http = ctx.serenity_context().http.clone();

    loop {
        let interaction = message
            .await_component_interaction(ctx.serenity_context().shard.clone())
            .timeout(Duration::from_secs(60))
            .await;

        match interaction {
            Some(interaction) => {
                interaction.defer(&http).await
                    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

                let player = ctx.data().player.read().await;

                if player.queue.is_empty() {
                    drop(player);
                    let _ = message.edit(&http, EditMessage::new()
                        .embed(QueueEmbed::IsEmpty.to_embed())
                        .components(vec![])
                    ).await;
                    break;
                }

                let total_pages = (player.queue.len() + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE;

                match interaction.data.custom_id.as_str() {
                    "queue_prev" => page = page.saturating_sub(1).max(1),
                    "queue_next" => page = (page + 1).min(total_pages),
                    _ => {}
                }
                page = page.min(total_pages);

                let embed = QueueEmbed::Current { queue: &player.queue, page }.to_embed();
                drop(player);

                let _ = message.edit(&http, EditMessage::new()
                    .embed(embed)
                    .components(nav_buttons(page, total_pages))
                ).await;
            }
            None => {
                let _ = message.delete(&http).await;
                break;
            }
        }
    }

    Ok(())
}

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
