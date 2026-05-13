use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::sources::spotify::spotify_client::{SpotifyClient, SpotifyError, SpotifySearchResult};
use crate::sources::youtube::youtube_client::{SearchError, YouTubeSearchResult, YoutubeClient};
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage, Message};
use std::collections::HashMap;
use std::convert::Into;
use std::time::{Duration, Instant};
use tokio::sync::RwLockWriteGuard;

const YOUTUBE_VIDEO_URL: &str = "https://www.youtube.com/watch?v=";
const YOUTUBE_PLAYLIST_URL: &str = "https://www.youtube.com/playlist?list=";

/// Play a track or playlist from YouTube or Spotify.
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn play(ctx: Context<'_>, track_source: Vec<String>) -> Result<(), MusicBotError> {
    do_play(ctx, track_source.join(" "), false).await
}

/// Play a track or playlist immediately by inserting it at the front of the queue.
#[poise::command(
    prefix_command, slash_command,
    rename = "playtop",
    check = "check_author_in_same_voice_channel",
)]
pub async fn play_top(ctx: Context<'_>, track_source: Vec<String>) -> Result<(), MusicBotError> {
    do_play(ctx, track_source.join(" "), true).await
}

async fn do_play(ctx: Context<'_>, track_source: String, top: bool) -> Result<(), MusicBotError> {
    let mut result: Result<YouTubeSearchResult, SearchError> = Err(SearchError::InternalError("No search result found".into()));

    // Search YouTube
    if track_source.starts_with(YOUTUBE_VIDEO_URL) || track_source.starts_with(YOUTUBE_PLAYLIST_URL) {
        let youtube_client: &YoutubeClient = &ctx.data().youtube_client;

        if track_source.starts_with(YOUTUBE_VIDEO_URL) {
            result = youtube_client.search_track_url(track_source.clone(), 1).await;
        }
        else if track_source.starts_with(YOUTUBE_PLAYLIST_URL) {
            result = youtube_client.fetch_playlist_lazy(track_source.clone()).await;
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
        result = youtube_client.search_track_url(track_source.clone(), 5).await;
    }

    match result {
        Ok(YouTubeSearchResult::Track(mut track)) => {
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
            let mut buttons: Vec<CreateButton> = (0..tracks.len())
                .map(|i| {
                    CreateButton::new(format!("track_{}", i))
                        .label((i + 1).to_string())
                        .style(ButtonStyle::Secondary)
                })
                .collect();
            buttons.push(
                CreateButton::new("track_cancel")
                    .label("✖ Cancel")
                    .style(ButtonStyle::Danger),
            );

            let row_count = buttons.len().div_ceil(5);
            let per_row = buttons.len().div_ceil(row_count.max(1));
            let rows: Vec<CreateActionRow> = buttons
                .chunks(per_row.max(1))
                .map(|chunk| CreateActionRow::Buttons(chunk.to_vec()))
                .collect();

            let reply_handle = ctx.send(
                poise::CreateReply::default()
                    .embed(PlayerEmbed::Search(&tracks).to_embed())
                    .components(rows)
                    .reply(true)
            ).await
                .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

            let message: Message = reply_handle.into_message().await
                .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

            let deadline = Instant::now() + Duration::from_secs(60 * 2);
            let mut cooldowns: HashMap<serenity::all::UserId, Instant> = HashMap::new();
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    message.delete(ctx.http()).await?;
                    PlayerEmbed::SearchExpired
                        .to_embed()
                        .send_context(ctx, true, Some(30))
                        .await?;
                    return Ok(());
                }

                let interaction = message
                    .await_component_interaction(ctx.serenity_context().shard.clone())
                    .timeout(remaining)
                    .await;

                match interaction {
                    Some(interaction) => {
                        if interaction.user.id != ctx.author().id {
                            let now = Instant::now();
                            let on_cooldown = cooldowns.get(&interaction.user.id)
                                .map(|&last| now.duration_since(last) < Duration::from_secs(5))
                                .unwrap_or(false);
                            if on_cooldown {
                                interaction.defer(ctx.http()).await.ok();
                            } else {
                                cooldowns.insert(interaction.user.id, now);
                                interaction.create_response(ctx.http(), CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Only the person who ran this command can select a track.")
                                        .ephemeral(true)
                                )).await.ok();
                            }
                            continue;
                        }

                        interaction.defer(ctx.http()).await?;
                        message.delete(ctx.http()).await?;

                        if interaction.data.custom_id == "track_cancel" {
                            PlayerEmbed::SearchCancelled
                                .to_embed()
                                .send_context(ctx, true, Some(30))
                                .await?;
                            return Ok(());
                        }

                        let track_index: usize = interaction.data.custom_id
                            .strip_prefix("track_")
                            .and_then(|s| s.parse().ok())
                            .unwrap();
                        let mut track: Track = tracks.swap_remove(track_index);
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
                        break;
                    }
                    None => {
                        message.delete(ctx.http()).await?;
                        PlayerEmbed::SearchExpired
                            .to_embed()
                            .send_context(ctx, true, Some(30))
                            .await?;
                        return Ok(());
                    }
                }
            }
        }

        Ok(YouTubeSearchResult::Playlist(mut playlist)) => {
            let added_by = ctx.author().name.clone();
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

async fn report_playback_error(ctx: Context<'_>, error: crate::player::player::PlaybackError) -> Result<(), MusicBotError> {
    PlayerEmbed::PlaybackErrorEmbed(error.to_string())
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;
    Ok(())
}
