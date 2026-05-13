use std::path::PathBuf;
use tokio::process::Command;

const CACHE_DIR: &str = "cache/youtube";

pub fn cache_dir() -> PathBuf {
    PathBuf::from(CACHE_DIR)
}

async fn ensure_cache_dir() -> std::io::Result<PathBuf> {
    let dir = cache_dir();
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

fn cache_path(video_id: &str) -> PathBuf {
    cache_dir().join(format!("{}.opus", video_id))
}

/// Sanitize a track title for use in a filename.
fn sanitize_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    let trimmed = cleaned.trim().to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

/// Build the expected cache path for a video: `{title}_{video_id}.opus`.
pub fn named_cache_path(video_id: &str, title: &str) -> PathBuf {
    let safe_title = sanitize_title(title);
    cache_dir().join(format!("{}_{}.opus", safe_title, video_id))
}

/// Check whether a video is already cached, matching by the video ID suffix.
/// Returns the path if found.
pub async fn find_cached(video_id: &str) -> Option<PathBuf> {
    // First try the exact named path (fast path).
    let dir = cache_dir();
    let suffix = format!("_{}.opus", video_id);

    let mut read_dir = tokio::fs::read_dir(&dir).await.ok()?;
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(&suffix) || name == format!("{}.opus", video_id) {
                return Some(path);
            }
        }
    }
    None
}

/// Download a YouTube video's audio to the cache directory via yt-dlp.
/// Returns the path of the downloaded file.
async fn download_to_cache(video_id: &str, url: &str, title: &str) -> std::io::Result<PathBuf> {
    let dir = ensure_cache_dir().await?;
    let output_path = dir.join(format!("{}_{}.opus", sanitize_title(title), video_id));

    tracing::info!(
        "Downloading YouTube video {} to cache: {:?}",
        video_id,
        output_path
    );

    let status = Command::new("yt-dlp")
        .args([
            "-x",
            "--audio-format",
            "opus",
            "--audio-quality",
            "0",
            "--no-playlist",
            "--no-warnings",
            "-o",
            output_path.to_str().unwrap_or_default(),
            url,
        ])
        .status()
        .await?;

    if status.success() {
        tracing::info!("Cached video {} at {:?}", video_id, output_path);
        Ok(output_path)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("yt-dlp exited with code {:?}", status.code()),
        ))
    }
}

/// Return the cached file path for a YouTube video, downloading it first if
/// necessary.  Falls back gracefully to `None` so callers can stream live.
pub async fn get_or_cache(video_id: &str, url: &str, title: &str) -> Option<PathBuf> {
    if let Some(path) = find_cached(video_id).await {
        tracing::debug!("Cache hit for YouTube video {}", video_id);
        return Some(path);
    }

    match download_to_cache(video_id, url, title).await {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::warn!("Failed to cache YouTube video {}: {}", video_id, e);
            None
        }
    }
}

/// Legacy helper kept for the simple `{video_id}.opus` naming scheme.
#[allow(dead_code)]
pub async fn find_cached_by_id(video_id: &str) -> Option<PathBuf> {
    let path = cache_path(video_id);
    if tokio::fs::metadata(&path).await.is_ok() {
        Some(path)
    } else {
        None
    }
}
