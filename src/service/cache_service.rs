//! On-disk cache for tracks resolved through yt-dlp. Once a track has played
//! through, the audio is kept under `cache/<source>/<title>_<id>.<ext>`
//! (with `<source>` being `youtube` or `spotify`, and `<ext>` whatever native
//! container yt-dlp produced — usually `webm` or `m4a`) so subsequent plays
//! skip the YouTube fetch (and the API/quota hit that goes with it). The
//! project's symphonia decoder is built with `features = ["all"]`, so any
//! container yt-dlp picks plays back fine.
//!
//! Legacy flat `cache/<stem>.<ext>` files from before the split are still
//! discovered on read, so an existing cache survives the upgrade — only new
//! downloads land in the per-source folders.

use crate::player::track::{Track, TrackSource};
use crate::service::normalize_service;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

// `cache_track` is the only public write entrypoint now — callers spawn the
// task themselves so they can chain follow-up work (e.g. applying loudness
// normalization to the currently playing track once its cache is ready).

const CACHE_DIR: &str = "cache";
const YOUTUBE_SUBDIR: &str = "youtube";
const SPOTIFY_SUBDIR: &str = "spotify";
const MAX_FILENAME_STEM: usize = 80;

pub fn cache_dir() -> PathBuf {
    PathBuf::from(CACHE_DIR)
}

/// Per-source cache directory. `None` for tracks that aren't fetched (local
/// files), since we never write those to the cache.
pub fn cache_dir_for(source: &TrackSource) -> Option<PathBuf> {
    let sub = match source {
        TrackSource::YouTube => YOUTUBE_SUBDIR,
        TrackSource::Spotify => SPOTIFY_SUBDIR,
        TrackSource::Local(_) => return None,
    };
    Some(cache_dir().join(sub))
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
    if out.is_empty() {
        "track".to_string()
    } else {
        out
    }
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
///
/// Search order: per-source subdirectory first, then the legacy flat root so
/// pre-split caches keep working without a migration step.
pub async fn find_cached(track: &Track) -> Option<PathBuf> {
    let stem = cache_stem_for(track)?;

    if let Some(dir) = cache_dir_for(&track.source) {
        if let Some(path) = find_in_dir(&dir, &stem).await {
            return Some(path);
        }
    }

    find_in_dir(&cache_dir(), &stem).await
}

async fn find_in_dir(
    dir: &Path,
    stem: &str,
) -> Option<PathBuf> {
    let mut read_dir = tokio::fs::read_dir(dir).await.ok()?;
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
        // Skip sidecar files used by normalize_service (current + legacy).
        if rest == normalize_service::SIDECAR_EXT || rest == normalize_service::LEGACY_SIDECAR_EXT {
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
    let stem = cache_stem_for(track).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "track is not cacheable"))?;

    if let Some(existing) = find_cached(track).await {
        return Ok(existing);
    }

    let dir = cache_dir_for(&track.source).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "track has no cache directory",
        )
    })?;
    ensure_dir(&dir).await?;

    let input_url = track
        .metadata
        .play_url
        .clone()
        .unwrap_or_else(|| track.metadata.track_url.clone());

    // Write to `<stem>.part.<ext>` first so a half-downloaded file isn't
    // picked up by `find_cached` on a concurrent lookup.
    let output_template = dir.join(format!("{stem}.part.%(ext)s"));

    let output = Command::new("yt-dlp")
        .args(["--no-warnings", "--no-playlist", "-f", "bestaudio/best", "-o"])
        .arg(&output_template)
        .arg(&input_url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        cleanup_part_files(&dir, &stem).await;
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
    let mut read_dir = tokio::fs::read_dir(&dir).await?;
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
        let final_path = dir.join(format!("{stem}.{ext}"));
        tokio::fs::rename(entry.path(), &final_path).await?;
        return Ok(final_path);
    }

    Err(std::io::Error::other(
        "yt-dlp reported success but produced no output file",
    ))
}

/// Delete any leftover `<stem>.part.*` files in `dir`. Called when yt-dlp
/// fails so we don't accumulate partials on retry.
async fn cleanup_part_files(
    dir: &Path,
    stem: &str,
) {
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

/// Whether `track` has enough metadata to be cacheable. Used by callers
/// before they spawn a cache-and-apply background job so we don't kick off
/// work for tracks we can't cache (local files, or sources missing an id).
pub fn is_cacheable(track: &Track) -> bool {
    cache_stem_for(track).is_some()
}

/// Result of a single combined yt-dlp metadata probe.
pub struct TrackProbe {
    /// Finite duration when yt-dlp could determine one; `None` for livestreams,
    /// region-blocked content, or any probe failure.
    pub duration: Option<Duration>,
    /// `true` when yt-dlp explicitly reports the video as a live broadcast.
    pub is_live: bool,
}

/// Probe `track` with a single yt-dlp invocation, fetching both duration and
/// live-status at once. Returns `None` for local files (no probe needed).
pub async fn probe_track(track: &Track) -> Option<TrackProbe> {
    if matches!(track.source, TrackSource::Local(_)) {
        return None;
    }
    let input_url = track
        .metadata
        .play_url
        .clone()
        .unwrap_or_else(|| track.metadata.track_url.clone());

    let output = Command::new("yt-dlp")
        .args(["--no-warnings", "--no-playlist", "--print", "%(duration)s", "--print", "%(is_live)s"])
        .arg(&input_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let duration_line = lines.next().unwrap_or("").trim().to_owned();
    let is_live_line = lines.next().unwrap_or("").trim().to_owned();

    let duration = duration_line.parse::<f64>().ok().and_then(
        |s| {
            if s.is_finite() && s > 0.0 {
                Some(Duration::from_secs(s as u64))
            } else {
                None
            }
        },
    );
    let is_live = is_live_line.eq_ignore_ascii_case("true");

    Some(TrackProbe { duration, is_live })
}
