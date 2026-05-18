use crate::service::cache_service;
use crate::utils::ytdlp_utils::js_runtime_args;
use songbird::input::{File, Input, YoutubeDl};
use std::path::PathBuf;
use std::time::Duration;

/// Tracks longer than this are rejected outright — too long to be a music
/// queue item (typically full DJ sets, podcasts, lectures).
pub const MAX_TRACK_DURATION: Duration = Duration::from_secs(60 * 60);
/// Tracks longer than this are streamed but never cached to disk, so the
/// cache doesn't bloat with long-form audio that's unlikely to be replayed.
pub const STREAM_ONLY_DURATION: Duration = Duration::from_secs(60 * 10);

#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    // Bare display string — `MusicBotError::InternalError` already adds the
    // user-facing "Whoops…" prefix when this is converted at the boundary.
    #[error("{0}")]
    InternalError(String),

    #[error("No tracks in queue")]
    NoTracksInQueue,

    #[error("Playback is not active")]
    PlaybackNotActive,

    #[error("Playback is already active")]
    PlaybackAlreadyActive,

    #[error("Playback is already paused")]
    PlaybackAlreadyPaused,

    #[error("Playback is not paused")]
    PlaybackNotPaused,

    #[error("Invalid queue index: {0}")]
    InvalidQueueIndex(usize),
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub description: String,
    pub playlist_url: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: String,
    pub metadata: TrackMetadata,
    pub added_by: String,
    pub source: TrackSource,
}

#[derive(Debug, Clone)]
pub enum TrackSource {
    /// Streamed via yt-dlp from a YouTube URL.
    YouTube,
    /// Resolved from Spotify, played via yt-dlp's `ytsearch1:` prefix.
    Spotify,
    /// A previously downloaded file on the local filesystem.
    Local(PathBuf),
}

impl TrackSource {
    pub fn label(&self) -> &'static str {
        match self {
            TrackSource::YouTube => "YouTube",
            TrackSource::Spotify => "Spotify",
            TrackSource::Local(_) => "Local file",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            TrackSource::YouTube => "🎬",
            TrackSource::Spotify => "🟢",
            TrackSource::Local(_) => "📁",
        }
    }
}

impl Track {
    /// Pick the best input for this track:
    ///   1. If a raw cache exists, play that.
    ///   2. Else stream through yt-dlp; the caller is expected to kick off
    ///      a background cache-and-normalize pass via
    ///      `spawn_cache_and_apply` so the gain can be applied mid-track
    ///      as soon as the cache is ready.
    ///
    /// Returns the chosen `Input` along with the on-disk path it was built
    /// from (when available). The path is what loudness normalization needs;
    /// streamed inputs return `None`.
    pub async fn resolve_input(
        &self,
        req_client: &reqwest::Client,
    ) -> (Input, Option<PathBuf>) {
        if let TrackSource::Local(path) = &self.source {
            return (File::new(path.clone()).into(), Some(path.clone()));
        }

        if let Some(raw) = cache_service::find_cached(self).await {
            let path = raw.clone();
            return (File::new(raw).into(), Some(path));
        }

        // Cache miss: stream now. The caller fires off the cache write.
        // `play_url` lets sources (like Spotify) ship a yt-dlp-friendly input
        // string while keeping `track_url` set to the user-facing permalink.
        let input_url = self
            .metadata
            .play_url
            .clone()
            .unwrap_or_else(|| self.metadata.track_url.clone());
        let ytdl = YoutubeDl::new(req_client.clone(), input_url).user_args(js_runtime_args());
        (ytdl.into(), None)
    }
}

#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub track_url: String,
    /// Optional override used by `build_input`. For Spotify this is the
    /// `ytsearch1:` query, while `track_url` stays the Spotify permalink.
    pub play_url: Option<String>,
    /// Reported track length, when the source knew it at resolution time.
    /// `None` means we never asked — sources without cheap duration info
    /// (YouTube Data API search) leave this unset until a yt-dlp probe runs.
    pub duration: Option<Duration>,
}

impl Track {
    pub fn duration(&self) -> Option<Duration> {
        self.metadata.duration
    }

    /// True when the track is known to be over the hard length cap. Returns
    /// false if duration is unknown — callers should probe before assuming
    /// the track is acceptable.
    pub fn is_known_too_long(&self) -> bool {
        self.duration().is_some_and(|d| d > MAX_TRACK_DURATION)
    }

    /// True when the track is known to be too long for caching. Returns
    /// false if duration is unknown.
    pub fn is_known_long_form(&self) -> bool {
        self.duration().is_some_and(|d| d > STREAM_ONLY_DURATION)
    }
}
