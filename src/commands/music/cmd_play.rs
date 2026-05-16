use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::embeds::music::queue_embed::QueueEmbed;
use crate::player::player::Player;
use crate::player::track::{Track, MAX_TRACK_DURATION};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::service::picker_service::{self, PickerOutcome};
use crate::sources::spotify_player::{SpotifyClient, SpotifyError, SpotifySearchResult};
use crate::sources::youtube_player::{SearchError, YouTubeSearchResult, YoutubeClient};
use tokio::sync::RwLockWriteGuard;

const YOUTUBE_VIDEO_URL: &str = "https://www.youtube.com/watch?v=";
const YOUTUBE_PLAYLIST_URL: &str = "https://www.youtube.com/playlist?list=";

/// Play a track or playlist from YouTube or Spotify.
#[poise::command(
    prefix_command,
    slash_command,
    check = "check_author_in_same_voice_channel"
)]
pub async fn play(
    ctx: Context<'_>,
    track_source: Vec<String>,
) -> Result<(), MusicBotError> {
    do_play(ctx, track_source.join(" "), false).await
}

/// Play a track or playlist immediately by inserting it at the front of the queue.
#[poise::command(
    prefix_command,
    slash_command,
    rename = "playtop",
    check = "check_author_in_same_voice_channel"
)]
pub async fn play_top(
    ctx: Context<'_>,
    track_source: Vec<String>,
) -> Result<(), MusicBotError> {
    do_play(ctx, track_source.join(" "), true).await
}

async fn do_play(
    ctx: Context<'_>,
    track_source: String,
    top: bool,
) -> Result<(), MusicBotError> {
    let mut result: Result<YouTubeSearchResult, SearchError> = Err(SearchError::InternalError("No search result found".into()));

    // Search YouTube
    if track_source.starts_with(YOUTUBE_VIDEO_URL) || track_source.starts_with(YOUTUBE_PLAYLIST_URL) {
        let youtube_client: &YoutubeClient = &ctx.data().youtube_client;

        if track_source.starts_with(YOUTUBE_VIDEO_URL) {
            result = youtube_client
                .search_track_url(track_source.clone(), 1)
                .await;
        } else if track_source.starts_with(YOUTUBE_PLAYLIST_URL) {
            result = youtube_client
                .fetch_playlist_lazy(track_source.clone())
                .await;
        }
    }
    // Search Spotify
    else if SpotifyClient::is_spotify_url(&track_source) {
        let spotify_client = &ctx.data().spotify_client;

        match spotify_client.search(&track_source).await {
            Ok(SpotifySearchResult::Track(track)) => {
                result = Ok(YouTubeSearchResult::Track(track));
            }
            Ok(SpotifySearchResult::Playlist(playlist)) => {
                result = Ok(YouTubeSearchResult::Playlist(playlist));
            }
            Err(SpotifyError::TrackNotFound(_)) | Err(SpotifyError::PlaylistNotFound(_)) => {
                result = Err(SearchError::VideoNotFound(track_source.clone()));
            }
            Err(error) => {
                return Err(MusicBotError::from(error));
            }
        }
    }
    // Search using text on YouTube
    else {
        let youtube_client: &YoutubeClient = &ctx.data().youtube_client;
        result = youtube_client
            .search_track_url(track_source.clone(), 5)
            .await;
    }

    match result {
        Ok(YouTubeSearchResult::Track(mut track)) => {
            if track.is_known_too_long() {
                PlayerEmbed::TrackTooLong {
                    title: track.metadata.title.clone(),
                    cap: MAX_TRACK_DURATION,
                }
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
                return Ok(());
            }

            track.added_by = ctx.author().name.clone();
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

            // Skip the "added to queue" confirmation when nothing is playing —
            // the new track will start immediately and NowPlaying covers it.
            if player.is_playing {
                QueueEmbed::TrackAdded(&track)
                    .to_embed()
                    .send_context(ctx, true, Some(30))
                    .await?;
            }

            if let Err(error) = player.add_track_to_queue(ctx, track.clone(), top).await {
                drop(player);
                report_playback_error(ctx, error).await?;
                return Ok(());
            }
            drop(player);
            channel_service::join_user_channel(ctx).await?;
        }

        Ok(YouTubeSearchResult::Tracks(mut tracks)) => {
            let outcome = picker_service::show_picker(
                ctx,
                tracks.len(),
                "track",
                PlayerEmbed::Search(&tracks).to_embed(),
                "Only the person who ran this command can select a track.",
            )
            .await?;

            match outcome {
                PickerOutcome::Selected(track_index) => {
                    let mut track: Track = tracks.swap_remove(track_index);
                    if track.is_known_too_long() {
                        PlayerEmbed::TrackTooLong {
                            title: track.metadata.title.clone(),
                            cap: MAX_TRACK_DURATION,
                        }
                        .to_embed()
                        .send_context(ctx, true, Some(30))
                        .await?;
                        return Ok(());
                    }
                    track.added_by = ctx.author().name.clone();

                    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

                    if player.is_playing {
                        QueueEmbed::TrackAdded(&track)
                            .to_embed()
                            .send_context(ctx, true, Some(30))
                            .await?;
                    }

                    if let Err(error) = player.add_track_to_queue(ctx, track, top).await {
                        drop(player);
                        report_playback_error(ctx, error).await?;
                        return Ok(());
                    }
                    drop(player);
                    channel_service::join_user_channel(ctx).await?;
                }
                PickerOutcome::Cancelled => {
                    PlayerEmbed::SearchCancelled
                        .to_embed()
                        .send_context(ctx, true, Some(30))
                        .await?;
                    return Ok(());
                }
                PickerOutcome::Expired => return Ok(()),
            }
        }

        Ok(YouTubeSearchResult::Playlist(mut playlist)) => {
            let added_by = ctx.author().name.clone();
            // Strip out tracks already known to exceed the length cap (Spotify
            // and yt-dlp lazy playlists carry duration; YouTube Data API does
            // not, so those slip through and get gated again at playback).
            playlist.tracks.retain(|t| !t.is_known_too_long());
            for track in &mut playlist.tracks {
                track.added_by = added_by.clone();
            }

            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

            QueueEmbed::PlaylistAdded(&playlist)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;

            if let Err(error) = player.add_playlist_to_queue(ctx, playlist, top).await {
                drop(player);
                report_playback_error(ctx, error).await?;
                return Ok(());
            }
            drop(player);
            channel_service::join_user_channel(ctx).await?;
        }

        Err(SearchError::VideoNotFound(_)) | Err(SearchError::PlaylistNotFound(_)) => {
            PlayerEmbed::NoResults(track_source)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
        }

        Err(SearchError::QuotaExceeded) => {
            PlayerEmbed::QuotaExceeded
                .to_embed()
                .send_context(ctx, true, Some(60))
                .await?;
        }

        Err(error) => {
            return Err(MusicBotError::from(error));
        }
    }

    Ok(())
}

async fn report_playback_error(
    ctx: Context<'_>,
    error: crate::player::track::PlaybackError,
) -> Result<(), MusicBotError> {
    PlayerEmbed::PlaybackErrorEmbed(error.to_string())
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;
    Ok(())
}
