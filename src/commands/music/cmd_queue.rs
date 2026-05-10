use crate::bot::{Context, MusicBotError};
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, EditMessage};
use std::time::Duration;
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

/// List upcoming tracks in the queue.
#[poise::command(
    prefix_command, slash_command,
)]
pub async fn queue(ctx: Context<'_>, page: Option<usize>) -> Result<(), MusicBotError> {
    let player: RwLockReadGuard<Player> = ctx.data().player.read().await;

    // Treat the embed as empty only when there's no current track AND no queue.
    // A paused/playing track on its own should still render in the queue embed.
    if player.queue.is_empty() && player.current_track.is_none() {
        drop(player);
        QueueEmbed::IsEmpty
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    let total_pages = ((player.queue.len() + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE).max(1);
    let mut page = page.unwrap_or(1).max(1).min(total_pages);

    // No pagination needed for a single page
    if total_pages <= 1 {
        QueueEmbed::Current {
            now_playing: player.current_track.as_ref(),
            queue: &player.queue,
            page,
        }
            .to_embed()
            .send_context(ctx, true, Some(60))
            .await?;
        return Ok(());
    }

    let embed = QueueEmbed::Current {
        now_playing: player.current_track.as_ref(),
        queue: &player.queue,
        page,
    }.to_embed();
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

                if player.queue.is_empty() && player.current_track.is_none() {
                    drop(player);
                    let _ = message.edit(&http, EditMessage::new()
                        .embed(QueueEmbed::IsEmpty.to_embed())
                        .components(vec![])
                    ).await;
                    break;
                }

                let total_pages = ((player.queue.len() + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE).max(1);

                match interaction.data.custom_id.as_str() {
                    "queue_prev" => page = page.saturating_sub(1).max(1),
                    "queue_next" => page = (page + 1).min(total_pages),
                    _ => {}
                }
                page = page.min(total_pages);

                let embed = QueueEmbed::Current {
                    now_playing: player.current_track.as_ref(),
                    queue: &player.queue,
                    page,
                }.to_embed();
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
