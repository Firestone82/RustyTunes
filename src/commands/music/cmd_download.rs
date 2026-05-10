use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::{Player, Track, TrackMetadata, TrackSource};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::local_service;
use serenity::all::Attachment;
use std::path::PathBuf;
use tokio::sync::RwLockWriteGuard;

/// Download an audio file (URL or attachment) and play it.
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn download(
    ctx: Context<'_>,
    #[description = "Audio file to upload"] file: Option<Attachment>,
    #[description = "URL of the audio file to download"]
    #[rest]
    url: Option<String>,
) -> Result<(), MusicBotError> {
    let source = match resolve_source(ctx, file, url) {
        Some(s) => s,
        None => {
            PlayerEmbed::DownloadFailed(
                "Provide a URL or attach an audio file to your message.".to_string(),
            )
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
            return Ok(());
        }
    };

    PlayerEmbed::Downloading(source.display_label())
        .to_embed()
        .send_context(ctx, true, Some(15))
        .await?;

    let path = match save_to_library(ctx, &source).await {
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

    let track = build_local_track(path, ctx.author().name.clone());
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

/// What we're pulling into the library. Either an attached Discord file
/// (which already carries a content type and filename) or a raw URL we have
/// to inspect after the fact.
enum DownloadSource {
    Attachment {
        url: String,
        filename: String,
        content_type: Option<String>,
    },
    Url(String),
}

impl DownloadSource {
    fn display_label(&self) -> &str {
        match self {
            DownloadSource::Attachment { filename, .. } => filename,
            DownloadSource::Url(url) => url,
        }
    }

    fn url(&self) -> &str {
        match self {
            DownloadSource::Attachment { url, .. } => url,
            DownloadSource::Url(url) => url,
        }
    }
}

fn resolve_source(
    ctx: Context<'_>,
    file: Option<Attachment>,
    url: Option<String>,
) -> Option<DownloadSource> {
    if let Some(att) = file {
        return Some(DownloadSource::Attachment {
            url: att.url,
            filename: att.filename,
            content_type: att.content_type,
        });
    }

    // Prefix-command users may simply attach a file without using the slash
    // option, so fall back to the message's first attachment.
    if let poise::Context::Prefix(prefix) = ctx {
        if let Some(att) = prefix.msg.attachments.first() {
            return Some(DownloadSource::Attachment {
                url: att.url.clone(),
                filename: att.filename.clone(),
                content_type: att.content_type.clone(),
            });
        }
    }

    url.map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .map(DownloadSource::Url)
}

async fn save_to_library(
    ctx: Context<'_>,
    source: &DownloadSource,
) -> Result<PathBuf, MusicBotError> {
    let url = source.url();

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(MusicBotError::InternalError(
            "URL must start with http:// or https://".to_string(),
        ));
    }

    let dir = local_service::ensure_downloads_dir().await
        .map_err(|e| MusicBotError::InternalError(format!("Could not create downloads dir: {e}")))?;

    let response = ctx.data().request_client.get(url).send().await
        .map_err(|e| MusicBotError::InternalError(format!("Request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(MusicBotError::InternalError(format!(
            "Server returned {}", response.status()
        )));
    }

    let filename = match source {
        DownloadSource::Attachment { filename, content_type, .. } => {
            if !is_audio(filename, content_type.as_deref()) {
                return Err(MusicBotError::InternalError(format!(
                    "Attachment `{filename}` doesn't look like an audio file."
                )));
            }
            let mut name = local_service::sanitize_filename(filename);
            // Discord allows audio files without recognized extensions; add
            // one so `!local` can find the file later.
            if !local_service::has_audio_extension(&name) {
                let ext = audio_ext_from_content_type(content_type.as_deref())
                    .unwrap_or("mp3");
                name = format!("{name}.{ext}");
            }
            name
        }
        DownloadSource::Url(_) => filename_from_response(url, &response),
    };

    let target = local_service::unique_path(&dir, &filename).await;

    let bytes = response.bytes().await
        .map_err(|e| MusicBotError::InternalError(format!("Failed to read body: {e}")))?;

    tokio::fs::write(&target, &bytes).await
        .map_err(|e| MusicBotError::InternalError(format!("Failed to write file: {e}")))?;

    Ok(target)
}

fn is_audio(filename: &str, content_type: Option<&str>) -> bool {
    if local_service::has_audio_extension(filename) {
        return true;
    }
    content_type
        .map(|ct| ct.starts_with("audio/"))
        .unwrap_or(false)
}

fn audio_ext_from_content_type(ct: Option<&str>) -> Option<&'static str> {
    let ct = ct?.split(';').next()?.trim().to_ascii_lowercase();
    Some(match ct.as_str() {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/opus" => "opus",
        _ => return None,
    })
}

fn filename_from_response(url: &str, response: &reqwest::Response) -> String {
    if let Some(disposition) = response.headers().get(reqwest::header::CONTENT_DISPOSITION) {
        if let Ok(value) = disposition.to_str() {
            if let Some(name) = parse_content_disposition_filename(value) {
                let cleaned = local_service::sanitize_filename(&name);
                if local_service::has_audio_extension(&cleaned) {
                    return cleaned;
                }
            }
        }
    }
    local_service::filename_from_url(url)
}

fn parse_content_disposition_filename(header: &str) -> Option<String> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename=") {
            return Some(rest.trim_matches('"').to_string());
        }
        if let Some(rest) = part.strip_prefix("filename*=") {
            // Format: filename*=UTF-8''actual%20name.mp3
            if let Some(idx) = rest.find("''") {
                let raw = &rest[idx + 2..];
                return Some(percent_decode(raw));
            }
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn build_local_track(path: PathBuf, added_by: String) -> Track {
    let title = local_service::track_title(&path);
    let id = path.to_string_lossy().to_string();
    let display_url = format!("file://{}", path.to_string_lossy());

    Track {
        id: id.clone(),
        metadata: TrackMetadata {
            id,
            title,
            channel: "Local file".to_string(),
            track_url: display_url,
        },
        added_by,
        source: TrackSource::Local(path),
    }
}
