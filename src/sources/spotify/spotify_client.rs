use crate::player::player::{Playlist, Track, TrackMetadata};
use crate::sources::youtube::youtube_client::{SearchError, YouTubeSearchResult, YoutubeClient};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use dotenv::var;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const SPOTIFY_API: &str = "https://api.spotify.com/v1";

const SPOTIFY_TRACK_URL: &str = "https://open.spotify.com/track/";
const SPOTIFY_PLAYLIST_URL: &str = "https://open.spotify.com/playlist/";

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

impl From<SearchError> for SpotifyError {
    fn from(value: SearchError) -> Self {
        SpotifyError::ResolveError(value.to_string())
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
    name: String,
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

#[derive(Deserialize)]
struct SpPlaylistTracks {
    items: Vec<SpPlaylistItem>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize)]
struct SpPlaylistItem {
    track: Option<SpTrack>,
}

#[derive(Deserialize)]
struct SpPagedTracks {
    items: Vec<SpPlaylistItem>,
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

    pub fn is_spotify_url(url: &str) -> bool {
        url.starts_with(SPOTIFY_TRACK_URL) || url.starts_with(SPOTIFY_PLAYLIST_URL)
    }

    fn extract_id(url: &str, prefix: &str) -> String {
        let rest = url.trim_start_matches(prefix);
        rest.split(['?', '/', '#']).next().unwrap_or(rest).to_string()
    }

    async fn access_token(&self) -> Result<String, SpotifyError> {
        let id = self.client_id.as_ref().ok_or(SpotifyError::NotConfigured)?;
        let secret = self.client_secret.as_ref().ok_or(SpotifyError::NotConfigured)?;

        let mut guard = self.token.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > Instant::now() + Duration::from_secs(15) {
                return Ok(cached.token.clone());
            }
        }

        let basic = BASE64.encode(format!("{id}:{secret}"));
        let response = self.http
            .post(SPOTIFY_TOKEN_URL)
            .header("Authorization", format!("Basic {basic}"))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpotifyError::ApiError(format!("token request failed: {status} {body}")));
        }

        let token: TokenResponse = response.json().await?;
        let cached = CachedToken {
            token: token.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(token.expires_in),
        };
        *guard = Some(cached);
        Ok(token.access_token)
    }

    async fn fetch_track(&self, id: &str) -> Result<SpTrack, SpotifyError> {
        let token = self.access_token().await?;
        let response = self.http
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
            return Err(SpotifyError::ApiError(format!("track fetch failed: {status} {body}")));
        }

        Ok(response.json().await?)
    }

    async fn fetch_playlist(&self, id: &str) -> Result<SpPlaylist, SpotifyError> {
        let token = self.access_token().await?;
        let response = self.http
            .get(format!("{SPOTIFY_API}/playlists/{id}"))
            .bearer_auth(token)
            .query(&[("fields", "id,name,description,tracks(items(track(id,name,artists(name))),next)")])
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(SpotifyError::PlaylistNotFound(id.to_string()));
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpotifyError::ApiError(format!("playlist fetch failed: {status} {body}")));
        }

        Ok(response.json().await?)
    }

    async fn fetch_playlist_page(&self, next_url: &str) -> Result<SpPagedTracks, SpotifyError> {
        let token = self.access_token().await?;
        let response = self.http
            .get(next_url)
            .bearer_auth(token)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SpotifyError::ApiError(format!("playlist page failed: {status} {body}")));
        }
        Ok(response.json().await?)
    }

    pub async fn search(
        &self,
        url: &str,
        youtube: &YoutubeClient,
    ) -> Result<SpotifySearchResult, SpotifyError> {
        if url.starts_with(SPOTIFY_TRACK_URL) {
            let id = Self::extract_id(url, SPOTIFY_TRACK_URL);
            let track = self.fetch_track(&id).await?;
            let resolved = resolve_via_youtube(youtube, &track).await?;
            Ok(SpotifySearchResult::Track(resolved))
        } else if url.starts_with(SPOTIFY_PLAYLIST_URL) {
            let id = Self::extract_id(url, SPOTIFY_PLAYLIST_URL);
            let playlist = self.fetch_playlist(&id).await?;

            let mut sp_tracks: Vec<SpTrack> = playlist.tracks.items
                .into_iter()
                .filter_map(|item| item.track)
                .filter(|t| t.id.is_some())
                .collect();

            // Cap pagination so we don't hammer YouTube quota; mirrors YT 50 max.
            let mut next = playlist.tracks.next;
            while let Some(url) = next {
                if sp_tracks.len() >= 50 {
                    break;
                }
                let page = self.fetch_playlist_page(&url).await?;
                sp_tracks.extend(
                    page.items.into_iter()
                        .filter_map(|item| item.track)
                        .filter(|t| t.id.is_some())
                );
                next = page.next;
            }
            sp_tracks.truncate(50);

            let mut tracks: Vec<Track> = Vec::with_capacity(sp_tracks.len());
            for sp in sp_tracks {
                match resolve_via_youtube(youtube, &sp).await {
                    Ok(t) => tracks.push(t),
                    Err(e) => tracing::warn!("Skipping Spotify track '{}': {}", sp.name, e),
                }
            }

            if tracks.is_empty() {
                return Err(SpotifyError::PlaylistNotFound(format!(
                    "No playable tracks resolved for playlist {id}"
                )));
            }

            Ok(SpotifySearchResult::Playlist(Playlist {
                id: playlist.id.clone(),
                title: playlist.name,
                description: playlist.description,
                playlist_url: format!("{SPOTIFY_PLAYLIST_URL}{}", playlist.id),
                tracks,
            }))
        } else {
            Err(SpotifyError::ApiError(format!("Unsupported Spotify URL: {url}")))
        }
    }
}

fn track_query(track: &SpTrack) -> String {
    let artists = track.artists.iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if artists.is_empty() {
        track.name.clone()
    } else {
        format!("{} - {}", artists, track.name)
    }
}

async fn resolve_via_youtube(youtube: &YoutubeClient, sp: &SpTrack) -> Result<Track, SpotifyError> {
    let query = track_query(sp);
    match youtube.search_track_url(query.clone(), 1).await? {
        YouTubeSearchResult::Track(mut t) => {
            // Override title/channel with Spotify metadata so the user sees what they asked for.
            t.metadata = TrackMetadata {
                id: t.metadata.id,
                title: sp.name.clone(),
                channel: sp.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "),
                track_url: t.metadata.track_url,
            };
            Ok(t)
        }
        YouTubeSearchResult::Tracks(mut tracks) => {
            if tracks.is_empty() {
                Err(SpotifyError::ResolveError(format!("No YouTube match for '{query}'")))
            } else {
                let mut t = tracks.swap_remove(0);
                t.metadata.title = sp.name.clone();
                t.metadata.channel = sp.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", ");
                Ok(t)
            }
        }
        YouTubeSearchResult::Playlist(_) => {
            Err(SpotifyError::ResolveError("Unexpected playlist result".to_string()))
        }
    }
}
