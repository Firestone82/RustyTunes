use crate::player::track::{Playlist, Track, TrackMetadata};
use crate::utils::yt_dlp_utils;
use dotenv::var;
use google_youtube3::api::{PlaylistItem, PlaylistItemSnippet, SearchResult, SearchResultSnippet};
use google_youtube3::client::NoToken;
use google_youtube3::hyper::client::HttpConnector;
use google_youtube3::hyper_rustls::HttpsConnector;
use google_youtube3::YouTube;
use html_escape::decode_html_entities;
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct YoutubeClient {
    api_key: String,
    youtube: YouTube<HttpsConnector<HttpConnector>>,
}

pub enum YouTubeSearchResult {
    Track(Track),
    Tracks(Vec<Track>),
    Playlist(Playlist),
}

#[derive(thiserror::Error, Debug)]
pub enum SearchError {
    // Display strings here are bare so they don't double-up with
    // `MusicBotError::InternalError`'s "Whoops…" prefix.
    #[error("{0}")]
    InternalError(String),

    #[error("API thrown error: {0}")]
    ApiError(String),

    #[error("Video not found: {0}")]
    VideoNotFound(String),

    #[error("Playlist not found: {0}")]
    PlaylistNotFound(String),

    #[error("YouTube API quota exceeded — please try again later or contact the bot owner.")]
    QuotaExceeded,
}

const SINGLE_URI: &str = "https://www.youtube.com/watch?v=";
const PLAYLIST_URI: &str = "https://www.youtube.com/playlist?list=";

/// Convert a YouTube Data API error into the right `SearchError` variant.
/// The 403 quota-exceeded payload is buried inside the response body string,
/// so we sniff for it and surface a friendly variant.
fn map_api_error<E: std::fmt::Display>(error: E) -> SearchError {
    let message = error.to_string();
    if message.contains("quotaExceeded") || message.contains("youtube.quota") {
        return SearchError::QuotaExceeded;
    }
    SearchError::ApiError(message)
}

impl Default for YoutubeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl YoutubeClient {
    pub fn new() -> Self {
        let youtube_token = var("YOUTUBE_TOKEN").expect("Expected a valid youtube token set in the configuration.");

        let connector = google_youtube3::hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .unwrap()
            .https_or_http()
            .enable_http1()
            .build();

        let client = google_youtube3::hyper::Client::builder().build(connector);

        Self {
            api_key: youtube_token,
            youtube: YouTube::new(client, NoToken),
        }
    }

    pub async fn search_track_url(
        &self,
        url: String,
        max_tracks: u32,
    ) -> Result<YouTubeSearchResult, SearchError> {
        let request = self
            .youtube
            .search()
            .list(&vec![String::from("id"), String::from("snippet")])
            .q(&url)
            .param("key", &self.api_key)
            .add_type("video")
            .max_results(max_tracks);

        let (_, response) = request.doit().await.map_err(map_api_error)?;

        let items: Vec<SearchResult> = response
            .items
            .ok_or_else(|| SearchError::VideoNotFound(format!("No video found for url: {}", url)))?;

        let mut tracks: Vec<Track> = items
            .iter()
            .filter_map(|result| {
                let video_id: String = result.id.as_ref()?.video_id.clone()?;

                let snippet: &SearchResultSnippet = result.snippet.as_ref()?;
                let title: &String = snippet.title.as_ref()?;
                let channel: &String = snippet.channel_title.as_ref()?;

                let metadata: TrackMetadata = TrackMetadata {
                    id: video_id.clone(),
                    title: decode_html_entities(title).to_string(),
                    channel: decode_html_entities(channel).to_string(),
                    track_url: format!("{SINGLE_URI}{}", video_id),
                    play_url: None,
                    // YouTube Data API search/playlist responses don't carry
                    // duration; we'd need a separate videos.list call. Probed
                    // lazily via yt-dlp before playback instead.
                    duration: None,
                };

                Some(Ok(Track {
                    id: video_id,
                    metadata,
                    added_by: String::new(),
                    source: crate::player::track::TrackSource::YouTube,
                }))
            })
            .collect::<Result<Vec<Track>, SearchError>>()?;

        if tracks.is_empty() {
            return Err(SearchError::VideoNotFound(format!(
                "No usable results for: {url}"
            )));
        }

        match max_tracks {
            1 => Ok(YouTubeSearchResult::Track(tracks.swap_remove(0))),
            _ => Ok(YouTubeSearchResult::Tracks(tracks)),
        }
    }

    pub async fn search_playlist_url(
        &self,
        url: String,
    ) -> Result<YouTubeSearchResult, SearchError> {
        let playlist_id: &str = url.trim_start_matches(PLAYLIST_URI);

        let playlist_request = self
            .youtube
            .playlists()
            .list(&vec![String::from("id"), String::from("snippet")])
            .add_id(playlist_id)
            .param("key", &self.api_key)
            .max_results(1);

        let (_, response) = playlist_request.doit().await.map_err(map_api_error)?;

        if let Some(playlist) = response.items {
            if playlist.is_empty() {
                return Err(SearchError::PlaylistNotFound(format!(
                    "No playlist found for url: {}",
                    url
                )));
            }

            let snippet = playlist.first().unwrap().snippet.as_ref().unwrap();

            let title: &String = snippet.title.as_ref().unwrap();
            let description: &String = snippet.description.as_ref().unwrap();

            let tracks_request = self
                .youtube
                .playlist_items()
                .list(&vec![String::from("id"), String::from("snippet")])
                .playlist_id(playlist_id)
                .param("key", &self.api_key)
                .max_results(50);

            let (_, response) = tracks_request.doit().await.map_err(map_api_error)?;

            let items: Vec<PlaylistItem> = response
                .items
                .ok_or_else(|| SearchError::VideoNotFound(format!("No video found for url: {}", url)))?;

            let tracks: Vec<Track> = items
                .iter()
                .filter_map(|result| {
                    let video_id: String = result
                        .snippet
                        .as_ref()?
                        .resource_id
                        .clone()?
                        .video_id
                        .clone()?;

                    let snippet: &PlaylistItemSnippet = result.snippet.as_ref()?;
                    let title: &String = snippet.title.as_ref()?;
                    let channel: &String = snippet.channel_title.as_ref()?;

                    let metadata: TrackMetadata = TrackMetadata {
                        id: video_id.clone(),
                        title: decode_html_entities(title).to_string(),
                        channel: decode_html_entities(channel).to_string(),
                        track_url: format!("{SINGLE_URI}{}", video_id),
                        play_url: None,
                        duration: None,
                    };

                    Some(Ok(Track {
                        id: video_id,
                        metadata,
                        added_by: String::new(),
                        source: crate::player::track::TrackSource::YouTube,
                    }))
                })
                .collect::<Result<Vec<Track>, SearchError>>()?;

            Ok(YouTubeSearchResult::Playlist(Playlist {
                id: playlist_id.to_string(),
                title: decode_html_entities(title).to_string(),
                description: decode_html_entities(description).to_string(),
                playlist_url: format!("{PLAYLIST_URI}{}", playlist_id),
                tracks,
            }))
        } else {
            Err(SearchError::PlaylistNotFound(format!(
                "No playlist found for url: {}",
                url
            )))
        }
    }

    /// Enumerate a YouTube playlist using yt-dlp's `--flat-playlist` mode.
    /// Streams results line-by-line so we get track titles immediately from
    /// yt-dlp scraping — zero YouTube Data API quota used.
    pub async fn fetch_playlist_lazy(
        &self,
        url: String,
    ) -> Result<YouTubeSearchResult, SearchError> {
        let playlist_id = url.trim_start_matches(PLAYLIST_URI).to_string();

        let mut child = tokio::process::Command::new("yt-dlp")
            .args(yt_dlp_utils::extra_args())
            .args([
                "--flat-playlist",
                "--no-warnings",
                "--print",
                "%j", // one JSON object per line
                &url,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| SearchError::InternalError(format!("Failed to spawn yt-dlp: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SearchError::InternalError("yt-dlp stdout missing".into()))?;

        let mut lines = BufReader::new(stdout).lines();

        let mut playlist_title = String::new();
        let mut playlist_desc = String::new();
        let mut tracks: Vec<Track> = Vec::new();

        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(v): Result<Value, _> = serde_json::from_str(&line) else {
                continue;
            };

            // Grab playlist metadata from the first entry.
            if tracks.is_empty() {
                if let Some(t) = v["playlist_title"].as_str().filter(|s| !s.is_empty()) {
                    playlist_title = t.to_string();
                }
                if let Some(d) = v["playlist_description"].as_str() {
                    playlist_desc = d.to_string();
                }
            }

            let Some(id) = v["id"].as_str() else { continue };
            let title = v["title"].as_str().unwrap_or(id).to_string();
            let channel = v["channel"]
                .as_str()
                .or_else(|| v["uploader"].as_str())
                .unwrap_or("")
                .to_string();
            let duration = v["duration"]
                .as_f64()
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(|d| std::time::Duration::from_secs(d as u64));

            tracks.push(Track {
                id: id.to_string(),
                metadata: TrackMetadata {
                    id: id.to_string(),
                    title,
                    channel,
                    track_url: format!("{SINGLE_URI}{id}"),
                    play_url: None,
                    duration,
                },
                added_by: String::new(),
                source: crate::player::track::TrackSource::YouTube,
            });
        }

        // Reap the child process.
        let _ = child.wait().await;

        if tracks.is_empty() {
            return Err(SearchError::PlaylistNotFound(format!(
                "No tracks found in playlist: {url}"
            )));
        }

        if playlist_title.is_empty() {
            playlist_title = format!("Playlist {playlist_id}");
        }

        Ok(YouTubeSearchResult::Playlist(Playlist {
            id: playlist_id.clone(),
            title: playlist_title,
            description: playlist_desc,
            playlist_url: format!("{PLAYLIST_URI}{playlist_id}"),
            tracks,
        }))
    }
}
