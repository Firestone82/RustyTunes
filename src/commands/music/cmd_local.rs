use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::commands::music::cmd_download::{build_local_track, save_to_library, DownloadSource};
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::embeds::music::queue_embed::QueueEmbed;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::picker_service::{self, PickerOutcome};
use crate::sources::local::local_client;
use serenity::all::Attachment;
use std::path::PathBuf;
use tokio::sync::RwLockWriteGuard;

const PICKER_LIMIT: usize = 25;

/// Manage the local audio library (download, upload, list, play, rename, remove).
#[poise::command(
    prefix_command,
    slash_command,
    subcommands("download", "upload", "list", "remove", "play", "rename_track"),
    check = "check_author_in_same_voice_channel"
)]
pub async fn local(ctx: Context<'_>) -> Result<(), MusicBotError> {
    // Default action when called without a subcommand: list saved tracks.
    list_inner(ctx).await
}

/// Autocomplete from the current local library — used by `play` and `rename`.
async fn autocomplete_local_track(_ctx: Context<'_>, partial: &str) -> Vec<String> {
    let needle = partial.trim().to_ascii_lowercase();
    let files = local_client::list_local_files().await.unwrap_or_default();
    files
        .into_iter()
        .map(|p| local_client::track_title(&p))
        .filter(|title| needle.is_empty() || title.to_ascii_lowercase().contains(&needle))
        .take(25)
        .collect()
}

/// Download an audio file from a URL into the local library.
#[poise::command(prefix_command, slash_command)]
pub async fn download(
    ctx: Context<'_>,
    #[description = "URL of the audio file to download"] url: String,
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
    #[description = "Audio file to upload"] file: Option<Attachment>,
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

    let normalized_name = name.as_deref().map(|n| n.trim()).filter(|n| !n.is_empty());

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
    #[autocomplete = "autocomplete_local_track"]
    #[rest]
    name: Option<String>,
) -> Result<(), MusicBotError> {
    let needle = name
        .as_deref()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());

    let matches: Vec<PathBuf> = match needle.as_deref() {
        Some(q) => local_client::search_local(q)
            .await
            .map_err(|e| MusicBotError::InternalError(format!("Could not read downloads: {e}")))?,
        None => local_client::list_local_files()
            .await
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
    let picked = match pick_path(ctx, &display, "play", PlayerEmbed::LocalPickToPlay(&display))
        .await?
    {
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

    let matches: Vec<PathBuf> = local_client::search_local(needle)
        .await
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
        match pick_path(
            ctx,
            &display,
            "remove",
            PlayerEmbed::LocalPickToRemove(&display),
        )
        .await?
        {
            Some(p) => p,
            None => return Ok(()),
        }
    };

    let display_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();

    if let Err(e) = local_client::delete_local(&target).await {
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

/// Rename a downloaded track (extension preserved if omitted).
#[poise::command(rename = "rename", prefix_command, slash_command)]
pub async fn rename_track(
    ctx: Context<'_>,
    #[description = "Existing track name"]
    #[autocomplete = "autocomplete_local_track"]
    old: String,
    #[description = "New name (extension optional)"]
    #[rest]
    new: String,
) -> Result<(), MusicBotError> {
    let old_query = old.trim();
    let new_name = new.trim();

    if old_query.is_empty() {
        return reply_failure(ctx, "Provide the current track name.").await;
    }
    if new_name.is_empty() {
        return reply_failure(ctx, "Provide a new name.").await;
    }

    let target: PathBuf = match resolve_unique(ctx, old_query).await? {
        Some(p) => p,
        None => return Ok(()),
    };

    let current_ext = target.extension().and_then(|e| e.to_str()).unwrap_or("mp3");

    let cleaned = local_client::sanitize_filename(new_name);
    let new_filename = if local_client::has_audio_extension(&cleaned) {
        cleaned
    } else {
        format!("{cleaned}.{current_ext}")
    };

    let parent = target
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(local_client::downloads_dir);

    let new_path = local_client::unique_path(&parent, &new_filename).await;

    if let Err(e) = tokio::fs::rename(&target, &new_path).await {
        PlayerEmbed::DownloadFailed(format!("Rename failed: {e}"))
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    let old_display = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let new_display = new_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();

    PlayerEmbed::LocalRenamed {
        old: &old_display,
        new: &new_display,
    }
    .to_embed()
    .send_context(ctx, true, Some(30))
    .await?;
    Ok(())
}

/// Resolve a query to a single library file. Prefers an exact (case-insensitive)
/// title match — autocomplete-picked names will hit this path. Falls back to
/// substring search; if substring is ambiguous, shows the candidates and
/// returns `None`.
async fn resolve_unique(ctx: Context<'_>, query: &str) -> Result<Option<PathBuf>, MusicBotError> {
    let matches: Vec<PathBuf> = local_client::search_local(query)
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Could not read downloads: {e}")))?;

    if matches.is_empty() {
        PlayerEmbed::LocalNoMatch(query)
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await?;
        return Ok(None);
    }

    let needle_lower = query.trim().to_ascii_lowercase();
    if let Some(exact) = matches
        .iter()
        .find(|p| local_client::track_title(p).to_ascii_lowercase() == needle_lower)
    {
        return Ok(Some(exact.clone()));
    }

    if matches.len() == 1 {
        return Ok(Some(matches.into_iter().next().unwrap()));
    }

    let display: Vec<PathBuf> = matches.into_iter().take(PICKER_LIMIT).collect();
    PlayerEmbed::LocalAmbiguous(&display)
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;
    Ok(None)
}

async fn list_inner(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let files: Vec<PathBuf> = local_client::list_local_files()
        .await
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

/// Show the path picker and return the chosen path. `None` on cancel/timeout.
async fn pick_path(
    ctx: Context<'_>,
    files: &[PathBuf],
    id_prefix: &str,
    embed: PlayerEmbed<'_>,
) -> Result<Option<PathBuf>, MusicBotError> {
    let outcome = picker_service::show_picker(
        ctx,
        files.len(),
        id_prefix,
        embed.to_embed(),
        "Only the person who ran this command can make a selection.",
    )
    .await?;

    match outcome {
        PickerOutcome::Selected(i) => Ok(files.get(i).cloned()),
        PickerOutcome::Cancelled => {
            PlayerEmbed::SearchCancelled
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            Ok(None)
        }
        PickerOutcome::Expired => Ok(None),
    }
}
