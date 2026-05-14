//! On-disk cache for tracks resolved through yt-dlp. Once a track has played
//! through, the audio is kept under `cache/` keyed by `<title>_<id>.<ext>`
//! (with `<ext>` being whatever native container yt-dlp produced — usually
//! `webm` or `m4a`) so subsequent plays skip the YouTube fetch (and the API/
//! quota hit that goes with it). The project's symphonia decoder is built
//! with `features = ["all"]`, so any container yt-dlp picks plays back fine.

use crate::player::player::{Track, TrackSource};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

const CACHE_DIR: &str = "cache";
const MAX_FILENAME_STEM: usize = 80;

pub fn cache_dir() -> PathBuf {
    PathBuf::from(CACHE_DIR)
}

async fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let mut out = String::new();
    for c in trimmed.chars() {
        if out.chars().count() >= MAX_FILENAME_STEM {
            break;
        }
        out.push(c);
    }
    if out.is_empty() { "track".to_string() } else { out }
}

/// Stem identifying `track` in the cache (without an extension). `None` means
/// the track isn't a fetched source (e.g. local files) or it lacks a usable id.
pub fn cache_stem_for(track: &Track) -> Option<String> {
    match &track.source {
        TrackSource::YouTube | TrackSource::Spotify => {
            let id = sanitize(&track.metadata.id);
            if id.is_empty() {
                return None;
            }
            let title = sanitize(&track.metadata.title);
            Some(format!("{title}_{id}"))
        }
        TrackSource::Local(_) => None,
    }
}

/// Look up `track` in the cache, ignoring extension. We don't pin a single
/// extension because `--audio-format opus` requires ffmpeg with libopus, which
/// isn't a given on every host — letting yt-dlp keep whatever container it
/// downloads (webm/m4a/opus/…) avoids a hard dep on a libopus-built ffmpeg.
pub async fn find_cached(track: &Track) -> Option<PathBuf> {
    let stem = cache_stem_for(track)?;
    let dir = cache_dir();
    let mut read_dir = tokio::fs::read_dir(&dir).await.ok()?;
    let prefix = format!("{stem}.");
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !name_str.starts_with(&prefix) {
            continue;
        }
        let rest = &name_str[prefix.len()..];
        // Skip half-written downloads (`<stem>.part.<ext>`).
        if rest.starts_with("part.") || rest == "part" {
            continue;
        }
        if !rest.contains('.') && !rest.is_empty() {
            return Some(entry.path());
        }
    }
    None
}

/// Download `track` through yt-dlp into the cache, returning the final path.
/// No-op (returns existing path) if a cached copy already exists.
pub async fn cache_track(track: &Track) -> std::io::Result<PathBuf> {
    let stem = cache_stem_for(track).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "track is not cacheable")
    })?;

    if let Some(existing) = find_cached(track).await {
        return Ok(existing);
    }

    ensure_dir(&cache_dir()).await?;

    let input_url = track
        .metadata
        .play_url
        .clone()
        .unwrap_or_else(|| track.metadata.track_url.clone());

    // Write to `<stem>.part.<ext>` first so a half-downloaded file isn't
    // picked up by `find_cached` on a concurrent lookup.
    let output_template = cache_dir().join(format!("{stem}.part.%(ext)s"));

    let output = Command::new("yt-dlp")
        .args(["--no-warnings", "--no-playlist", "-f", "bestaudio/best", "-o"])
        .arg(&output_template)
        .arg(&input_url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        cleanup_part_files(&cache_dir(), &stem).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail = stderr
            .lines()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(std::io::Error::other(format!(
            "yt-dlp failed ({}): {}",
            output.status, tail
        )));
    }

    // yt-dlp picked the extension based on whatever stream it grabbed; find
    // the produced file and rename it to drop the `.part` infix.
    let part_prefix = format!("{stem}.part.");
    let mut read_dir = tokio::fs::read_dir(cache_dir()).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let name = entry.file_name();
        let name_str = match name.to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !name_str.starts_with(&part_prefix) {
            continue;
        }
        let ext = &name_str[part_prefix.len()..];
        if ext.is_empty() || ext.contains('.') {
            // Unexpected double extension — skip and let cleanup catch it.
            continue;
        }
        let final_path = cache_dir().join(format!("{stem}.{ext}"));
        tokio::fs::rename(entry.path(), &final_path).await?;
        return Ok(final_path);
    }

    Err(std::io::Error::other(
        "yt-dlp reported success but produced no output file",
    ))
}

/// Delete any leftover `<stem>.part.*` files in `dir`. Called when yt-dlp
/// fails so we don't accumulate partials on retry.
async fn cleanup_part_files(dir: &Path, stem: &str) {
    let part_prefix = format!("{stem}.part.");
    let mut read_dir = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return,
    };
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name();
        if let Some(s) = name.to_str() {
            if s.starts_with(&part_prefix) {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }
}

/// Fire-and-forget caching of `track` after playback has already started.
/// Errors are logged but never surfaced — a cache miss next time is fine.
pub fn spawn_cache(track: Track) {
    if cache_stem_for(&track).is_none() {
        return;
    }
    tokio::spawn(async move {
        match cache_track(&track).await {
            Ok(path) => tracing::info!(
                "Cached '{}' to {}",
                track.metadata.title,
                path.display()
            ),
            Err(e) => tracing::warn!(
                "Failed to cache '{}': {}",
                track.metadata.title,
                e
            ),
        }
    });
}
