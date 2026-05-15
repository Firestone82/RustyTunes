use crate::player::track::{Playlist, Track, TrackMetadata};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use dotenv::var;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const SPOTIFY_API: &str = "https://api.spotify.com/v1";

const SPOTIFY_PLAYLIST_URL: &str = "https://open.spotify.com/playlist/";

#[derive(Debug, Clone, Copy)]
pub enum SpotifyKind {
    Track,
    Playlist,
}

pub enum SpotifySearchResult {
    Track(Track),
    Playlist(Playlist),
}

#[derive(thiserror::Error, Debug)]
pub enum SpotifyError {
    #[error("Spotify is not configured. Set SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET.")]
    NotConfigured,

    #[error("Spotify API error: {0}")]
    ApiError(String),

    #[error("Track not found: {0}")]
    TrackNotFound(String),

    #[error("Playlist not found: {0}")]
    PlaylistNotFound(String),

    #[error("Unable to resolve track on YouTube: {0}")]
    ResolveError(String),
}

impl From<reqwest::Error> for SpotifyError {
    fn from(value: reqwest::Error) -> Self {
        SpotifyError::ApiError(value.to_string())
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct SpArtist {
    name: String,
}

#[derive(Deserialize)]
struct SpTrack {
    id: Option<String>,
    // Some podcast episodes or restricted items may omit these fields.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    artists: Vec<SpArtist>,
}

#[derive(Deserialize)]
struct SpPlaylist {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    tracks: SpPlaylistTracks,
}

// `items` are kept as raw JSON so a single malformed entry (podcast episodes
// with unexpected shapes, locally-uploaded files, region-restricted tracks)
// can be skipped individually instead of aborting the whole page parse.
// Both fields default so an empty `{}` response is handled gracefully.
#[derive(Deserialize)]
struct SpPlaylistTracks {
    #[serde(default)]
    items: Vec<JsonValue>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize)]
struct SpPagedTracks {
    #[serde(default)]
    items: Vec<JsonValue>,
    #[serde(default)]
    next: Option<String>,
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

pub struct SpotifyClient {
    client_id: Option<String>,
    client_secret: Option<String>,
    http: reqwest::Client,
    token: Arc<Mutex<Option<CachedToken>>>,
}

impl Default for SpotifyClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotifyClient {
    pub fn new() -> Self {
        let client_id = var("SPOTIFY_CLIENT_ID").ok().filter(|s| !s.is_empty());
        let client_secret = var("SPOTIFY_CLIENT_SECRET").ok().filter(|s| !s.is_empty());

        if client_id.is_none() || client_secret.is_none() {
            tracing::warn!("Spotify credentials not configured; Spotify URLs will be rejected");
        }

        Self {
            client_id,
            client_secret,
            http: reqwest::Client::new(),
            token: Arc::new(Mutex::new(None)),
        }
    }

    pub fn parse_url(url: &str) -> Option<(SpotifyKind, String)> {
        // Matches:
        //   https://open.spotify.com/track/<id>
        //   https://open.spotify.com/intl-en/track/<id>
        //   https://open.spotify.com/playlist/<id>?si=...
        //   spotify:track:<id>
        //   spotify:playlist:<id>
        let re = Regex::new(r"(?:https?://open\.spotify\.com/(?:intl-[a-zA-Z-]+/)?(track|playlist)/|spotify:(track|playlist):)([A-Za-z0-9]+)").ok()?;
        let caps = re.captures(url)?;
        let kind_str = caps.get(1).or_else(|| caps.get(2))?.as_str();
        let id = caps.get(3)?.as_str().to_string();
        let kind = match kind_str {
            "track" => SpotifyKind::Track,
            "playlist" => SpotifyKind::Playlist,
            _ => return None,
        };
        Some((kind, id))
    }

    pub fn is_spotify_url(url: &str) -> bool {
        Self::parse_url(url).is_some()
    }

    async fn access_token(&self) -> Result<String, SpotifyError> {
        let id = self.client_id.as_ref().ok_or(SpotifyError::NotConfigured)?;
        let secret = self
            .client_secret
            .as_ref()
            .ok_or(SpotifyError::NotConfigured)?;

        let mut guard = self.token.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > Instant::now() + Duration::from_secs(15) {
                return Ok(cached.token.clone());
            }
        }

        let basic = BASE64.encode(format!("{id}:{secret}"));
        let response = self
            .http
            .post(SPOTIFY_TOKEN_URL)
            .header("Authorization", format!("Basic {basic}"))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpotifyError::ApiError(format!(
                "token request failed: {status} {body}"
            )));
        }

        let token: TokenResponse = response.json().await?;
        let cached = CachedToken {
            token: token.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(token.expires_in),
        };
        *guard = Some(cached);
        Ok(token.access_token)
    }

    async fn fetch_track(
        &self,
        id: &str,
    ) -> Result<SpTrack, SpotifyError> {
        let token = self.access_token().await?;
        let response = self
            .http
            .get(format!("{SPOTIFY_API}/tracks/{id}"))
            .bearer_auth(token)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(SpotifyError::TrackNotFound(id.to_string()));
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpotifyError::ApiError(format!(
                "track fetch failed: {status} {body}"
            )));
        }

        Ok(response.json().await?)
    }

    async fn fetch_playlist(
        &self,
        id: &str,
    ) -> Result<SpPlaylist, SpotifyError> {
        let token = self.access_token().await?;
        let response = self
            .http
            .get(format!("{SPOTIFY_API}/playlists/{id}"))
            .bearer_auth(token)
            // No `fields` filter: serde ignores unknown fields, and keeping the
            // filter out ensures the `next` URL Spotify generates has no
            // embedded field constraints that break when fetched standalone.
            .query(&[("limit", "100")])
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(SpotifyError::PlaylistNotFound(id.to_string()));
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(SpotifyError::ApiError(format!(
                "playlist fetch failed: {status} {body}"
            )));
        }

        serde_json::from_str(&body).map_err(|e| {
            tracing::error!("Failed to decode playlist response: {e}");
            SpotifyError::ApiError(format!("decode playlist failed: {e}"))
        })
    }

    async fn fetch_playlist_page(
        &self,
        next_url: &str,
    ) -> Result<SpPagedTracks, SpotifyError> {
        tracing::debug!("Fetching playlist page: {next_url}");
        let token = self.access_token().await?;
        let response = self.http.get(next_url).bearer_auth(token).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(SpotifyError::ApiError(format!(
                "playlist page failed: {status} {body}"
            )));
        }
        serde_json::from_str(&body).map_err(|e| {
            tracing::error!(
                "Failed to decode playlist page: {e}; body snippet: {}",
                body.chars().take(400).collect::<String>()
            );
            SpotifyError::ApiError(format!("decode playlist page failed: {e}"))
        })
    }

    pub async fn search(
        &self,
        url: &str,
    ) -> Result<SpotifySearchResult, SpotifyError> {
        let (kind, id) = Self::parse_url(url).ok_or_else(|| SpotifyError::ApiError(format!("Unsupported Spotify URL: {url}")))?;

        match kind {
            SpotifyKind::Track => {
                let track = self.fetch_track(&id).await?;
                Ok(SpotifySearchResult::Track(build_track(&track)))
            }
            SpotifyKind::Playlist => {
                let playlist = self.fetch_playlist(&id).await?;

                let mut sp_tracks: Vec<SpTrack> = extract_tracks(playlist.tracks.items);

                // Walk the `next` link until the API stops handing them out so
                // we pull the entire playlist instead of just the first 100.
                let mut next = playlist.tracks.next;
                while let Some(url) = next {
                    let page = self.fetch_playlist_page(&url).await?;
                    sp_tracks.extend(extract_tracks(page.items));
                    next = page.next;
                }

                let tracks: Vec<Track> = sp_tracks.iter().map(build_track).collect();

                if tracks.is_empty() {
                    return Err(SpotifyError::PlaylistNotFound(format!(
                        "Playlist {id} has no playable tracks"
                    )));
                }

                Ok(SpotifySearchResult::Playlist(Playlist {
                    id: playlist.id.clone(),
                    title: playlist.name,
                    description: playlist.description,
                    playlist_url: format!("{SPOTIFY_PLAYLIST_URL}{}", playlist.id),
                    tracks,
                }))
            }
        }
    }
}

// Pull `track` out of each playlist item and best-effort convert to SpTrack.
// Anything that fails (episodes with unexpected shapes, malformed entries) is
// logged at debug and skipped so one bad row doesn't kill the whole playlist.
fn extract_tracks(items: Vec<JsonValue>) -> Vec<SpTrack> {
    items
        .into_iter()
        .filter_map(|mut item| {
            let track = item.get_mut("track")?.take();
            if track.is_null() {
                return None;
            }
            match serde_json::from_value::<SpTrack>(track) {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::debug!("Skipping unparseable Spotify item: {e}");
                    None
                }
            }
        })
        .filter(|t| t.id.is_some() && t.name.as_deref().is_some_and(|n| !n.is_empty()))
        .collect()
}

fn track_query(track: &SpTrack) -> String {
    let name = track.name.as_deref().unwrap_or("");
    let artists = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if artists.is_empty() {
        name.to_string()
    } else {
        format!("{} - {}", artists, name)
    }
}

// Build a Track that yt-dlp resolves at playback time via its built-in
// `ytsearch1:` prefix. Avoids the YouTube Data API (and its 100-unit
// per-search quota cost) entirely — a Spotify playlist with many tracks
// would otherwise blow through the daily 10k quota almost immediately.
//
// `track_url` carries the Spotify permalink so embeds render a real link
// (or omit it gracefully when the API didn't return an id). `play_url`
// holds the `ytsearch1:` query that yt-dlp actually consumes.
fn build_track(sp: &SpTrack) -> Track {
    let query = track_query(sp);
    let channel = sp
        .artists
        .iter()
        .map(|a| a.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let title = sp.name.clone().unwrap_or_else(|| query.clone());
    let (id, track_url) = match &sp.id {
        Some(spotify_id) => (
            spotify_id.clone(),
            format!("https://open.spotify.com/track/{spotify_id}"),
        ),
        None => (query.clone(), String::new()),
    };
    Track {
        id: id.clone(),
        metadata: TrackMetadata {
            id,
            title,
            channel,
            track_url,
            play_url: Some(format!("ytsearch1:{query}")),
        },
        added_by: String::new(),
        source: crate::player::track::TrackSource::Spotify,
    }
}
