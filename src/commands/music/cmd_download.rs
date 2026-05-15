//! Helpers for fetching audio into the local library. Exposed for the
//! `local download` subcommand in `cmd_local`.

use crate::bot::{Context, MusicBotError};
use crate::player::track::{Track, TrackMetadata, TrackSource};
use crate::sources::local::local_client;
use std::path::PathBuf;

/// What we're pulling into the library. Either an attached Discord file
/// (which already carries a content type and filename) or a raw URL we have
/// to inspect after the fact.
pub enum DownloadSource {
    Attachment { url: String, filename: String, content_type: Option<String> },
    Url(String),
}

impl DownloadSource {
    pub fn display_label(&self) -> &str {
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

/// Download `source` into the downloads directory and return the saved path.
/// If `name_override` is given, the file is saved under that name (with the
/// extension preserved or inferred); otherwise we use the attachment filename
/// or derive one from the response/URL.
pub async fn save_to_library(ctx: Context<'_>, source: &DownloadSource, name_override: Option<&str>) -> Result<PathBuf, MusicBotError> {
    let url = source.url();

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(MusicBotError::InternalError("URL must start with http:// or https://".to_string()));
    }

    let dir = local_client::ensure_downloads_dir()
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Could not create downloads dir: {e}")))?;

    let response = ctx
        .data()
        .request_client
        .get(url)
        .send()
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(MusicBotError::InternalError(format!("Server returned {}", response.status())));
    }

    let auto_name = match source {
        DownloadSource::Attachment { filename, content_type, .. } => {
            if !is_audio(filename, content_type.as_deref()) {
                return Err(MusicBotError::InternalError(format!("Attachment `{filename}` doesn't look like an audio file.")));
            }
            let mut name = local_client::sanitize_filename(filename);
            // Discord allows audio files without recognized extensions; add
            // one so `local list` can find the file later.
            if !local_client::has_audio_extension(&name) {
                let ext = audio_ext_from_content_type(content_type.as_deref()).unwrap_or("mp3");
                name = format!("{name}.{ext}");
            }
            name
        }
        DownloadSource::Url(_) => filename_from_response(url, &response),
    };

    let filename = match name_override {
        Some(custom) => apply_name_override(custom, &auto_name),
        None => auto_name,
    };

    let target = local_client::unique_path(&dir, &filename).await;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Failed to read body: {e}")))?;

    tokio::fs::write(&target, &bytes)
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Failed to write file: {e}")))?;

    Ok(target)
}

/// Apply a user-supplied save name. The user's name takes precedence; we only
/// borrow the auto-detected extension if they didn't supply one of their own.
fn apply_name_override(custom: &str, auto_name: &str) -> String {
    let cleaned = local_client::sanitize_filename(custom);
    if local_client::has_audio_extension(&cleaned) {
        return cleaned;
    }
    let ext = extension_of(auto_name).unwrap_or("mp3");
    format!("{cleaned}.{ext}")
}

fn extension_of(name: &str) -> Option<&str> {
    let idx = name.rfind('.')?;
    if idx == 0 || idx == name.len() - 1 {
        return None;
    }
    Some(&name[idx + 1..])
}

fn is_audio(filename: &str, content_type: Option<&str>) -> bool {
    if local_client::has_audio_extension(filename) {
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
                let cleaned = local_client::sanitize_filename(&name);
                if local_client::has_audio_extension(&cleaned) {
                    return cleaned;
                }
            }
        }
    }
    local_client::filename_from_url(url)
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
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
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
    let title = local_client::track_title(&path);
    let id = path.to_string_lossy().to_string();
    let display_url = format!("file://{}", path.to_string_lossy());

    Track {
        id: id.clone(),
        metadata: TrackMetadata {
            id,
            title,
            channel: "Local file".to_string(),
            track_url: display_url,
            play_url: None,
        },
        added_by,
        source: TrackSource::Local(path),
    }
}
