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
use crate::sources::youtube_player::{SearchError, YouTubeSearchResult};
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

/// Skip the current song and immediately play a track or playlist from source.
#[poise::command(
    prefix_command,
    slash_command,
    rename = "playnow",
    check = "check_author_in_same_voice_channel"
)]
pub async fn play_now(
    ctx: Context<'_>,
    track_source: Vec<String>,
) -> Result<(), MusicBotError> {
    let source = track_source.join(" ");
    if source.trim().is_empty() {
        PlayerEmbed::MissingQuery
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    ctx.defer().await?;

    let result = resolve_source(ctx, &source).await?;

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
            if let Err(error) = player.force_play_track(ctx, track).await {
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
                    if let Err(error) = player.force_play_track(ctx, track).await {
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
            playlist.tracks.retain(|t| !t.is_known_too_long());
            for track in &mut playlist.tracks {
                track.added_by = added_by.clone();
            }
            QueueEmbed::PlaylistAdded(&playlist)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
            if let Err(error) = player.force_play_playlist(ctx, playlist).await {
                drop(player);
                report_playback_error(ctx, error).await?;
                return Ok(());
            }
            drop(player);
            channel_service::join_user_channel(ctx).await?;
        }

        Err(SearchError::VideoNotFound(_)) | Err(SearchError::PlaylistNotFound(_)) => {
            PlayerEmbed::NoResults(source)
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

async fn resolve_source(
    ctx: Context<'_>,
    track_source: &str,
) -> Result<Result<YouTubeSearchResult, SearchError>, MusicBotError> {
    if track_source.starts_with(YOUTUBE_VIDEO_URL) {
        let result = ctx
            .data()
            .youtube_client
            .search_track_url(track_source.to_owned(), 1)
            .await;
        return Ok(result);
    }

    if track_source.starts_with(YOUTUBE_PLAYLIST_URL) {
        let result = ctx
            .data()
            .youtube_client
            .fetch_playlist_lazy(track_source.to_owned())
            .await;
        return Ok(result);
    }

    if SpotifyClient::is_spotify_url(track_source) {
        let result = match ctx.data().spotify_client.search(track_source).await {
            Ok(SpotifySearchResult::Track(track)) => Ok(YouTubeSearchResult::Track(track)),
            Ok(SpotifySearchResult::Playlist(playlist)) => Ok(YouTubeSearchResult::Playlist(playlist)),
            Err(SpotifyError::TrackNotFound(_)) | Err(SpotifyError::PlaylistNotFound(_)) => Err(SearchError::VideoNotFound(track_source.to_owned())),
            Err(error) => return Err(MusicBotError::from(error)),
        };
        return Ok(result);
    }

    Ok(ctx
        .data()
        .youtube_client
        .search_track_url(track_source.to_owned(), 5)
        .await)
}

fn is_direct_url(source: &str) -> bool {
    source.starts_with(YOUTUBE_VIDEO_URL) || source.starts_with(YOUTUBE_PLAYLIST_URL) || SpotifyClient::is_spotify_url(source)
}

async fn do_play(
    ctx: Context<'_>,
    track_source: String,
    top: bool,
) -> Result<(), MusicBotError> {
    if track_source.trim().is_empty() {
        PlayerEmbed::MissingQuery
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    ctx.defer().await?;

    // For direct URLs the metadata fetch can take several seconds. Send an
    // acknowledgment now so the user sees something right away, before the
    // YouTube / Spotify API call finishes.
    if is_direct_url(&track_source) {
        PlayerEmbed::Queuing(&track_source)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    }

    let result = resolve_source(ctx, &track_source).await?;

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

            // Push first (infallible), confirm to the user, then kick off
            // playback. This order guarantees TrackAdded lands before
            // NowPlaying for an idle queue, and that we never show success
            // before a playback error — kick_off_playback is what can fail.
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
            player.push_track(track.clone(), top);

            QueueEmbed::TrackAdded(&track)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;

            if let Err(error) = player.kick_off_playback(ctx, top).await {
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
                    player.push_track(track.clone(), top);

                    QueueEmbed::TrackAdded(&track)
                        .to_embed()
                        .send_context(ctx, true, Some(30))
                        .await?;

                    if let Err(error) = player.kick_off_playback(ctx, top).await {
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
