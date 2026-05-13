//! On-disk cache for tracks resolved through yt-dlp. Once a track has played
//! through, the audio is kept under `cache/` keyed by `<title>_<id>.opus` so
//! subsequent plays skip the YouTube fetch (and the API/quota hit that goes
//! with it).
//!
//! Optional dynaudnorm-normalized siblings live under `cache/normalized/` and
//! are produced on demand when a guild has the session-only normalizer on.

use crate::player::player::{Track, TrackSource};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

const CACHE_DIR: &str = "cache";
const NORMALIZED_SUBDIR: &str = "normalized";
const CACHE_EXT: &str = "opus";
const MAX_FILENAME_STEM: usize = 80;

pub fn cache_dir() -> PathBuf {
    PathBuf::from(CACHE_DIR)
}

pub fn normalized_dir() -> PathBuf {
    cache_dir().join(NORMALIZED_SUBDIR)
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

/// Cache filename for `track`. `None` means the track isn't a fetched source
/// (e.g. local files don't go in cache) or it lacks a usable id.
pub fn cache_path_for(track: &Track) -> Option<PathBuf> {
    match &track.source {
        TrackSource::YouTube | TrackSource::Spotify => {
            let id = sanitize(&track.metadata.id);
            if id.is_empty() {
                return None;
            }
            let title = sanitize(&track.metadata.title);
            Some(cache_dir().join(format!("{title}_{id}.{CACHE_EXT}")))
        }
        TrackSource::Local(_) => None,
    }
}

/// Companion filename in `cache/normalized/` for `track`. Same naming as the
/// raw cache so the two stay one-to-one.
pub fn normalized_path_for(track: &Track) -> Option<PathBuf> {
    let raw = cache_path_for(track)?;
    let name = raw.file_name()?.to_owned();
    Some(normalized_dir().join(name))
}

pub async fn file_exists(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok()
}

/// Download `track` through yt-dlp into the cache, returning the final path.
/// No-op (returns existing path) if a cached copy already exists.
pub async fn cache_track(track: &Track) -> std::io::Result<PathBuf> {
    let cache_path = cache_path_for(track).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "track is not cacheable")
    })?;

    if file_exists(&cache_path).await {
        return Ok(cache_path);
    }

    ensure_dir(&cache_dir()).await?;

    let input_url = track
        .metadata
        .play_url
        .clone()
        .unwrap_or_else(|| track.metadata.track_url.clone());

    // yt-dlp + extract-audio always tacks `.opus` onto the stem; use a `.part`
    // stem so a concurrent run never picks up a half-written file.
    let stem = cache_path.with_extension("");
    let part_stem = {
        let mut s = stem.clone().into_os_string();
        s.push(".part");
        PathBuf::from(s)
    };
    let part_path = part_stem.with_extension(CACHE_EXT);

    let output_template = {
        let mut s = part_stem.clone().into_os_string();
        s.push(".%(ext)s");
        s
    };

    let status = Command::new("yt-dlp")
        .args([
            "--no-warnings",
            "--no-playlist",
            "-f",
            "bestaudio/best",
            "--extract-audio",
            "--audio-format",
            CACHE_EXT,
            "-o",
        ])
        .arg(&output_template)
        .arg(&input_url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;

    if !status.success() {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(std::io::Error::other(format!(
            "yt-dlp failed with status: {status}"
        )));
    }

    tokio::fs::rename(&part_path, &cache_path).await?;
    Ok(cache_path)
}

/// Produce a dynaudnorm-normalized sibling of `source` under
/// `cache/normalized/`. Already-normalized files are reused.
pub async fn normalize_file(source: &Path, target: &Path) -> std::io::Result<()> {
    if file_exists(target).await {
        return Ok(());
    }

    ensure_dir(&normalized_dir()).await?;

    let tmp = target.with_extension(format!("part.{CACHE_EXT}"));

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(source)
        .args([
            "-af",
            "dynaudnorm=f=200:g=15:p=0.95",
            "-c:a",
            "libopus",
            "-b:a",
            "128k",
        ])
        .arg(&tmp)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;

    if !status.success() {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(std::io::Error::other(format!(
            "ffmpeg failed with status: {status}"
        )));
    }

    tokio::fs::rename(&tmp, target).await?;
    Ok(())
}

/// Fire-and-forget caching of `track` after playback has already started.
/// Errors are logged but never surfaced — a cache miss next time is fine.
pub fn spawn_cache(track: Track) {
    if cache_path_for(&track).is_none() {
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
