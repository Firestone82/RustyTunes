use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::commands::music::cmd_download::{build_local_track, save_to_library, DownloadSource};
use crate::embeds::player_embed::PlayerEmbed;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::local_service;
use serenity::all::{Attachment, ButtonStyle, CreateActionRow, CreateButton, Message};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::RwLockWriteGuard;

const PICKER_LIMIT: usize = 25;

/// Manage the local audio library (download, upload, list, play, remove).
#[poise::command(
    prefix_command, slash_command,
    subcommands("download", "upload", "list", "remove", "play"),
    check = "check_author_in_same_voice_channel",
)]
pub async fn local(ctx: Context<'_>) -> Result<(), MusicBotError> {
    // Default action when called without a subcommand: list saved tracks.
    list_inner(ctx).await
}

/// Download an audio file from a URL into the local library.
#[poise::command(prefix_command, slash_command)]
pub async fn download(
    ctx: Context<'_>,
    #[description = "URL of the audio file to download"]
    url: String,
    #[description = "Save the file under this name"]
    #[rest]
    name: Option<String>,
) -> Result<(), MusicBotError> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return reply_failure(ctx, "Provide a URL.").await;
    }
    save_and_play(ctx, DownloadSource::Url(url), name).await
}

/// Upload an attached audio file into the local library.
#[poise::command(prefix_command, slash_command)]
pub async fn upload(
    ctx: Context<'_>,
    #[description = "Audio file to upload"]
    file: Option<Attachment>,
    #[description = "Save the file under this name"]
    #[rest]
    name: Option<String>,
) -> Result<(), MusicBotError> {
    // Slash users pass the attachment as an option; prefix users typically
    // just attach the file to the message itself.
    let attachment: Attachment = match file {
        Some(att) => att,
        None => match ctx {
            poise::Context::Prefix(prefix) => match prefix.msg.attachments.first() {
                Some(att) => att.clone(),
                None => return reply_failure(ctx, "Attach an audio file to your message.").await,
            },
            _ => return reply_failure(ctx, "Attach an audio file.").await,
        },
    };

    let source = DownloadSource::Attachment {
        url: attachment.url,
        filename: attachment.filename,
        content_type: attachment.content_type,
    };
    save_and_play(ctx, source, name).await
}

async fn save_and_play(
    ctx: Context<'_>,
    source: DownloadSource,
    name: Option<String>,
) -> Result<(), MusicBotError> {
    PlayerEmbed::Downloading(source.display_label())
        .to_embed()
        .send_context(ctx, true, Some(15))
        .await?;

    let normalized_name = name
        .as_deref()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty());

    let path = match save_to_library(ctx, &source, normalized_name).await {
        Ok(path) => path,
        Err(error) => {
            PlayerEmbed::DownloadFailed(error.to_string())
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
            return Ok(());
        }
    };

    let display_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_string();

    PlayerEmbed::Downloaded(&display_name)
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;

    enqueue_path(ctx, path).await
}

async fn reply_failure(ctx: Context<'_>, msg: &str) -> Result<(), MusicBotError> {
    PlayerEmbed::DownloadFailed(msg.to_string())
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;
    Ok(())
}

/// List previously downloaded tracks.
#[poise::command(prefix_command, slash_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), MusicBotError> {
    list_inner(ctx).await
}

/// Play a downloaded track by name. With no argument, picks from a list.
#[poise::command(prefix_command, slash_command)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Substring of the track name to play"]
    #[rest]
    name: Option<String>,
) -> Result<(), MusicBotError> {
    let needle = name
        .as_deref()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());

    let matches: Vec<PathBuf> = match needle.as_deref() {
        Some(q) => local_service::search_local(q).await
            .map_err(|e| MusicBotError::InternalError(format!("Could not read downloads: {e}")))?,
        None => local_service::list_local_files().await
            .map_err(|e| MusicBotError::InternalError(format!("Could not read downloads: {e}")))?,
    };

    if matches.is_empty() {
        match needle {
            Some(q) => {
                PlayerEmbed::LocalNoMatch(&q)
                    .to_embed()
                    .send_context(ctx, true, Some(15))
                    .await?;
            }
            None => {
                PlayerEmbed::LocalEmpty
                    .to_embed()
                    .send_context(ctx, true, Some(15))
                    .await?;
            }
        }
        return Ok(());
    }

    if matches.len() == 1 {
        return enqueue_path(ctx, matches.into_iter().next().unwrap()).await;
    }

    let display: Vec<PathBuf> = matches.into_iter().take(PICKER_LIMIT).collect();
    let picked = match show_picker(ctx, &display, "play", PlayerEmbed::LocalPickToPlay(&display)).await? {
        Some(p) => p,
        None => return Ok(()),
    };
    enqueue_path(ctx, picked).await
}

/// Remove a downloaded track by name.
#[poise::command(prefix_command, slash_command)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Substring of the track name to remove"]
    #[rest]
    name: String,
) -> Result<(), MusicBotError> {
    let needle = name.trim();
    if needle.is_empty() {
        PlayerEmbed::DownloadFailed("Provide a name to remove.".to_string())
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await?;
        return Ok(());
    }

    let matches: Vec<PathBuf> = local_service::search_local(needle).await
        .map_err(|e| MusicBotError::InternalError(format!("Could not read downloads: {e}")))?;

    if matches.is_empty() {
        PlayerEmbed::LocalNoMatch(needle)
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await?;
        return Ok(());
    }

    let target: PathBuf = if matches.len() == 1 {
        matches.into_iter().next().unwrap()
    } else {
        let display: Vec<PathBuf> = matches.into_iter().take(PICKER_LIMIT).collect();
        match show_picker(ctx, &display, "remove", PlayerEmbed::LocalPickToRemove(&display)).await? {
            Some(p) => p,
            None => return Ok(()),
        }
    };

    let display_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();

    if let Err(e) = local_service::delete_local(&target).await {
        PlayerEmbed::DownloadFailed(format!("Could not delete `{display_name}`: {e}"))
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    PlayerEmbed::LocalRemoved(&display_name)
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;
    Ok(())
}

async fn list_inner(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let files: Vec<PathBuf> = local_service::list_local_files().await
        .map_err(|e| MusicBotError::InternalError(format!("Could not read downloads: {e}")))?;

    if files.is_empty() {
        PlayerEmbed::LocalEmpty
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await?;
    } else {
        let display: Vec<PathBuf> = files.into_iter().take(PICKER_LIMIT).collect();
        PlayerEmbed::LocalFiles(&display)
            .to_embed()
            .send_context(ctx, true, Some(60))
            .await?;
    }
    Ok(())
}

async fn enqueue_path(ctx: Context<'_>, path: PathBuf) -> Result<(), MusicBotError> {
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
    Ok(())
}

/// Render numbered buttons for the supplied paths and wait for the user's
/// choice. Returns the picked path, or None if the user cancelled or the
/// selection timed out.
async fn show_picker(
    ctx: Context<'_>,
    files: &[PathBuf],
    id_prefix: &str,
    embed: PlayerEmbed<'_>,
) -> Result<Option<PathBuf>, MusicBotError> {
    let mut buttons: Vec<CreateButton> = (0..files.len())
        .map(|i| {
            CreateButton::new(format!("{id_prefix}_{i}"))
                .label((i + 1).to_string())
                .style(ButtonStyle::Secondary)
        })
        .collect();
    buttons.push(
        CreateButton::new(format!("{id_prefix}_cancel"))
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
            .embed(embed.to_embed())
            .components(rows)
            .reply(true)
    ).await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let message: Message = reply_handle.into_message().await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let interaction = message
        .await_component_interaction(ctx.serenity_context().shard.clone())
        .timeout(Duration::from_secs(60 * 2))
        .await;

    match interaction {
        Some(interaction) => {
            interaction.defer(ctx.http()).await?;
            message.delete(ctx.http()).await?;

            if interaction.data.custom_id == format!("{id_prefix}_cancel") {
                PlayerEmbed::SearchCancelled
                    .to_embed()
                    .send_context(ctx, true, Some(15))
                    .await?;
                return Ok(None);
            }

            let prefix = format!("{id_prefix}_");
            let index: usize = interaction.data.custom_id
                .strip_prefix(&prefix)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| MusicBotError::InternalError("Bad picker id".into()))?;

            Ok(files.get(index).cloned())
        }
        None => {
            message.delete(ctx.http()).await?;
            PlayerEmbed::SearchExpired
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            Ok(None)
        }
    }
}
