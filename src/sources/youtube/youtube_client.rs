use crate::player::player::{Track, TrackMetadata};
use dotenv::var;
use google_youtube3::client::NoToken;
use google_youtube3::hyper::client::HttpConnector;
use google_youtube3::hyper_rustls::HttpsConnector;
use google_youtube3::YouTube;
use html_escape::decode_html_entities;

pub struct YoutubeClient {
    api_key: String,
    youtube: YouTube<HttpsConnector<HttpConnector>>,
}

pub enum SearchResult {
    Track(Track),
    Tracks(Vec<Track>),
    Playlist(Vec<Track>),
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

    pub async fn search_track_url(&self, url: String, max_tracks: u32) -> Result<SearchResult, SearchError> {
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

        let (_, list) = request.doit().await
            .map_err(|error| SearchError::ApiError(error.to_string()))?;

        let results = list.items
            .ok_or_else(|| SearchError::VideoNotFound(url.clone()))?;

        if results.len() == 0 {
            return Err(SearchError::VideoNotFound(format!("No video found for url: {}", url)));
        }

        let tracks: Vec<Track> = results.iter().map(|result| {
            let video_id: Option<String> = result.id
                .as_ref()
                .and_then(|resource_id| resource_id.video_id.clone());

            let metadata: Option<TrackMetadata> = result.snippet.as_ref().and_then(|snippet| {
                let title: Option<&String> = snippet.title.as_ref();
                let channel: Option<&String> = snippet.channel_title.as_ref();
                let thumbnail_url: Option<&String> = snippet
                    .thumbnails
                    .as_ref()
                    .and_then(|details| {
                        details
                            .maxres
                            .as_ref()
                            .or(details.high.as_ref())
                            .or(details.medium.as_ref())
                            .or(details.standard.as_ref())
                            .or(details.default.as_ref())
                    })
                    .and_then(|thumbnail| thumbnail.url.as_ref());

                match (video_id.clone(), title, channel, thumbnail_url) {
                    (Some(video_id), Some(title), Some(channel), Some(thumbnail_url)) => Some(
                        TrackMetadata {
                            id: video_id.to_string(),
                            title: decode_html_entities(title).to_string(),
                            channel: decode_html_entities(channel).to_string(),
                            track_url: format!("{SINGLE_URI}{video_id}"),
                            thumbnail_url: thumbnail_url.to_string(),
                        }
                    ),
                    _ => None,
                }
            });

            match(video_id, metadata) {
                (Some(id), Some(metadata)) => Ok(
                    Track {
                        id,
                        metadata,
                    }
                ),
                _ => Err(SearchError::InternalError("Failed to parse video".to_owned()))
            }
        }).collect::<Result<Vec<Track>, SearchError>>()?;

        if max_tracks == 1 {
            // No fucking clue, why I need to move... p_p
            return Ok(SearchResult::Track(tracks[0].clone()));
        }

        Ok(SearchResult::Tracks(tracks))
    }

    pub async fn search_playlist_url(&self, url: String) -> Result<SearchResult, SearchError> {
        let playlist_id = url.trim_start_matches(PLAYLIST_URI);

        // let playlist_request = self.youtube
        //     .playlists()
        //     .list(&vec![
        //         String::from("id"),
        //         String::from("snippet"),
        //     ])
        //     .add_id(playlist_id)
        //     .param("key", &self.api_key)
        //     .max_results(1);
        //
        // let (_, playlist) = playlist_request.doit().await
        //     .map_err(|error| SearchError::ApiError(error.to_string()))?;

        let tracks_request = self.youtube
            .playlist_items()
            .list(&vec![
                String::from("id"),
                String::from("snippet"),
            ])
            .playlist_id(playlist_id)
            .param("key", &self.api_key)
            .max_results(50);

        let (_, list) = tracks_request.doit().await
            .map_err(|error| SearchError::ApiError(error.to_string()))?;

        let results = list.items
            .ok_or_else(|| SearchError::VideoNotFound(url.clone()))?;

        if results.len() == 0 {
            return Err(SearchError::VideoNotFound(format!("No video found for url: {}", url)));
        }

        let mut tracks: Vec<Track> = vec![];

        for result in results {
            
            let metadata: Option<TrackMetadata> = result.snippet.as_ref().and_then(|snippet| {
                let video_id: Option<String> = snippet.resource_id
                    .as_ref()
                    .and_then(|resource_id| resource_id.video_id.clone());

                let title: Option<&String> = snippet.title.as_ref();
                let channel: Option<&String> = snippet.channel_title.as_ref();
                let thumbnail_url: Option<&String> = snippet
                    .thumbnails
                    .as_ref()
                    .and_then(|details| {
                        details
                            .maxres
                            .as_ref()
                            .or(details.high.as_ref())
                            .or(details.medium.as_ref())
                            .or(details.standard.as_ref())
                            .or(details.default.as_ref())
                    })
                    .and_then(|thumbnail| thumbnail.url.as_ref());

                match (video_id.clone(), title, channel, thumbnail_url) {
                    (Some(video_id), Some(title), Some(channel), Some(thumbnail_url)) => Some(
                        TrackMetadata {
                            id: video_id.to_string(),
                            title: decode_html_entities(title).to_string(),
                            channel: decode_html_entities(channel).to_string(),
                            track_url: format!("{SINGLE_URI}{video_id}"),
                            thumbnail_url: thumbnail_url.to_string(),
                        }
                    ),
                    _ => None,
                }
            });

            // println!("{:?}", video_id);
            // println!("{:?}", metadata);
            // println!("{:?}/{:?}", PLAYLIST_URI, metadata);

            if let Some(metadata) = metadata {
                tracks.push(Track {
                    id: metadata.id.clone(),
                    metadata,
                });
            } else {
                println!("Failed to parse video - {:?}/{:?}", SINGLE_URI, result);
            }
        }

        Ok(SearchResult::Playlist(tracks))
    }

}