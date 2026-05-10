use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::commands::music::cmd_download::build_local_track;
use crate::embeds::player_embed::PlayerEmbed;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::local_service;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, Message};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::RwLockWriteGuard;

/// List previously downloaded tracks and pick one to play.
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn local(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let files: Vec<PathBuf> = local_service::list_local_files().await
        .map_err(|e| MusicBotError::InternalError(format!("Could not read downloads: {e}")))?;

    if files.is_empty() {
        PlayerEmbed::LocalEmpty
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await?;
        return Ok(());
    }

    // Discord caps at 25 buttons across 5 rows; 25 entries is plenty for a
    // local library list.
    let display: Vec<PathBuf> = files.into_iter().take(25).collect();

    let mut buttons: Vec<CreateButton> = (0..display.len())
        .map(|i| {
            CreateButton::new(format!("local_{}", i))
                .label((i + 1).to_string())
                .style(ButtonStyle::Secondary)
        })
        .collect();
    buttons.push(
        CreateButton::new("local_cancel")
            .label("✖ Cancel")
            .style(ButtonStyle::Danger),
    );

    let row_count = buttons.len().div_ceil(5);
    let per_row = buttons.len().div_ceil(row_count.max(1));
    let rows: Vec<CreateActionRow> = buttons
        .chunks(per_row.max(1))
        .map(|chunk| CreateActionRow::Buttons(chunk.to_vec()))
        .collect();

    let reply_handle = ctx.send(
        poise::CreateReply::default()
            .embed(PlayerEmbed::LocalFiles(&display).to_embed())
            .components(rows)
            .reply(true)
    ).await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let message: Message = reply_handle.into_message().await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let interaction = message
        .await_component_interaction(ctx.serenity_context().shard.clone())
        .timeout(Duration::from_secs(60 * 2));

    match interaction.await {
        Some(interaction) => {
            interaction.defer(ctx.http()).await?;
            message.delete(ctx.http()).await?;

            if interaction.data.custom_id == "local_cancel" {
                PlayerEmbed::SearchCancelled
                    .to_embed()
                    .send_context(ctx, true, Some(15))
                    .await?;
                return Ok(());
            }

            let index: usize = interaction.data.custom_id
                .strip_prefix("local_")
                .and_then(|s| s.parse().ok())
                .unwrap();

            let path: PathBuf = display[index].clone();
            let track: Track = build_local_track(path, ctx.author().name.clone());

            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

            if player.is_playing {
                QueueEmbed::TrackAdded(&track)
                    .to_embed()
                    .send_context(ctx, true, Some(30))
                    .await?;
            }

            if let Err(error) = player.add_track_to_queue(ctx, track, false).await {
                drop(player);
                PlayerEmbed::PlaybackErrorEmbed(error.to_string())
                    .to_embed()
                    .send_context(ctx, true, Some(30))
                    .await?;
                return Ok(());
            }
            drop(player);

            channel_service::join_user_channel(ctx).await?;
        }
        None => {
            message.delete(ctx.http()).await?;
            PlayerEmbed::SearchExpired
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
        }
    }

    Ok(())
}
