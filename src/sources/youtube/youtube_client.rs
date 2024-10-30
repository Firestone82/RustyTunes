use crate::player::player::{Playlist, Track, TrackMetadata};
use dotenv::var;
use google_youtube3::api::{PlaylistItem, PlaylistItemSnippet, SearchResult, SearchResultSnippet};
use google_youtube3::client::NoToken;
use google_youtube3::hyper::client::HttpConnector;
use google_youtube3::hyper_rustls::HttpsConnector;
use google_youtube3::YouTube;
use html_escape::decode_html_entities;

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
    #[error("Whoops, an internal error occurred: {0}")]
    InternalError(String),
    
    #[error("API thrown error: {0}")]
    ApiError(String),

    #[error("Video not found: {0}")]
    VideoNotFound(String),

    #[error("Playlist not found: {0}")]
    PlaylistNotFound(String),
}

const SINGLE_URI: &str = "https://www.youtube.com/watch?v=";
const PLAYLIST_URI: &str = "https://www.youtube.com/playlist?list=";

impl Default for YoutubeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl YoutubeClient {
    pub fn new() -> Self {
        let youtube_token = var("YOUTUBE_TOKEN")
            .expect("Expected a valid youtube token set in the configuration.");

        let connector = google_youtube3::hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .unwrap()
            .https_or_http()
            .enable_http1()
            .build();

        let client = google_youtube3::hyper::Client::builder()
            .build(connector);

        Self {
            api_key: youtube_token,
            youtube: YouTube::new(client, NoToken),
        }
    }

    pub async fn search_track_url(&self, url: String, max_tracks: u32) -> Result<YouTubeSearchResult, SearchError> {
        let request = self.youtube
            .search()
            .list(&vec![
                String::from("id"),
                String::from("snippet"),
            ])
            .q(&url)
            .param("key", &self.api_key)
            .add_type("video")
            .max_results(max_tracks);

        let (_, response) = request.doit()
            .await
            .map_err(|e| SearchError::ApiError(e.to_string()))?;

        let items: Vec<SearchResult> = response.items
            .ok_or_else(|| SearchError::VideoNotFound(format!("No video found for url: {}", url)))?;

        let tracks: Vec<Track> = items.iter()
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
                };

                Some(Ok(
                    Track {
                        id: video_id,
                        metadata
                    }
                ))
            })
            .collect::<Result<Vec<Track>, SearchError>>()?;

        match max_tracks {
            1 => {
                let track: &Track = tracks.iter()
                    .next()
                    .ok_or_else(|| SearchError::VideoNotFound(format!("No video found for url: {}", url)))?;

                Ok(YouTubeSearchResult::Track(track.clone()))
            }
            _ => Ok(YouTubeSearchResult::Tracks(tracks)),
        }
    }

    pub async fn search_playlist_url(&self, url: String) -> Result<YouTubeSearchResult, SearchError> {
        let playlist_id: &str = url.trim_start_matches(PLAYLIST_URI);
        
        let playlist_request = self.youtube
            .playlists()
            .list(&vec![
                String::from("id"),
                String::from("snippet"),
            ])
            .add_id(playlist_id)
            .param("key", &self.api_key)
            .max_results(1);
        
        let (_, response) = playlist_request.doit()
            .await
            .map_err(|e| SearchError::ApiError(e.to_string()))?;

        if let Some(playlist) = response.items {
            if playlist.is_empty() {
                return Err(SearchError::PlaylistNotFound(format!("No playlist found for url: {}", url)))
            }

            let snippet = playlist.first().unwrap().snippet.as_ref().unwrap();

            let title: &String = snippet.title.as_ref().unwrap();
            let description: &String = snippet.description.as_ref().unwrap();

            let tracks_request = self.youtube
                .playlist_items()
                .list(&vec![
                    String::from("id"),
                    String::from("snippet"),
                ])
                .playlist_id(playlist_id)
                .param("key", &self.api_key)
                .max_results(50);

            let (_, response) = tracks_request.doit()
                .await
                .map_err(|e| SearchError::ApiError(e.to_string()))?;

            let items: Vec<PlaylistItem> = response.items
                .ok_or_else(|| SearchError::VideoNotFound(format!("No video found for url: {}", url)))?;

            let tracks: Vec<Track> = items.iter()
                .filter_map(|result| {
                    let video_id: String = result.snippet.as_ref()?.resource_id.clone()?.video_id.clone()?;

                    let snippet: &PlaylistItemSnippet = result.snippet.as_ref()?;
                    let title: &String = snippet.title.as_ref()?;
                    let channel: &String = snippet.channel_title.as_ref()?;

                    let metadata: TrackMetadata = TrackMetadata {
                        id: video_id.clone(),
                        title: decode_html_entities(title).to_string(),
                        channel: decode_html_entities(channel).to_string(),
                        track_url: format!("{SINGLE_URI}{}", video_id),
                    };

                    Some(Ok(
                        Track {
                            id: video_id,
                            metadata
                        }
                    ))
                })
                .collect::<Result<Vec<Track>, SearchError>>()?;

            Ok(YouTubeSearchResult::Playlist(
                Playlist {
                    id: playlist_id.to_string(),
                    title: decode_html_entities(title).to_string(),
                    description: decode_html_entities(description).to_string(),
                    playlist_url: format!("{PLAYLIST_URI}{}", playlist_id),
                    tracks,
                }
            ))
        } else {
            Err(SearchError::PlaylistNotFound(format!("No playlist found for url: {}", url)))
        }
    }

}