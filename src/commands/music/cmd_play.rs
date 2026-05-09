use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::sources::youtube::youtube_client::{SearchError, YouTubeSearchResult, YoutubeClient};
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, Message};
use std::convert::Into;
use std::time::Duration;
use tokio::sync::RwLockWriteGuard;

const YOUTUBE_VIDEO_URL: &str = "https://www.youtube.com/watch?v=";
const YOUTUBE_PLAYLIST_URL: &str = "https://www.youtube.com/playlist?list=";

const SPOTIFY_TRACK_URL: &str = "https://open.spotify.com/track/";
const SPOTIFY_PLAYLIST_URL: &str = "https://open.spotify.com/playlist/";

/**
* Play a track or playlist from YouTube or Spotify
*/
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn play(ctx: Context<'_>, track_source: Vec<String>) -> Result<(), MusicBotError> {
    let track_source: String = track_source.join(" ");
    
    let mut result: Result<YouTubeSearchResult, SearchError> = Err(SearchError::InternalError("No search result found".into()));

    // Search YouTube
    if track_source.starts_with(YOUTUBE_VIDEO_URL) || track_source.starts_with(YOUTUBE_PLAYLIST_URL) {
        let youtube_client: &YoutubeClient = &ctx.data().youtube_client;

        if track_source.starts_with(YOUTUBE_VIDEO_URL) {
            result = youtube_client.search_track_url(track_source, 1).await;
        }
        else if track_source.starts_with(YOUTUBE_PLAYLIST_URL) {
            result = youtube_client.search_playlist_url(track_source).await;
        }
    }
    // Search Spotify
    else if track_source.starts_with(SPOTIFY_TRACK_URL) || track_source.starts_with(SPOTIFY_PLAYLIST_URL) {
        // TODO: Implement search for tracks using Spotify
    }
    // Search using text on YouTube
    else {
        let youtube_client: &YoutubeClient = &ctx.data().youtube_client;
        result = youtube_client.search_track_url(track_source, 5).await;
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

            player.add_track_to_queue(ctx, track.clone()).await?;
            channel_service::join_user_channel(ctx).await?;
        }

        Ok(YouTubeSearchResult::Tracks(mut tracks)) => {
            let buttons: Vec<CreateButton> = (0..tracks.len())
                .map(|i| {
                    CreateButton::new(format!("track_{}", i))
                        .label((i + 1).to_string())
                        .style(ButtonStyle::Secondary)
                })
                .collect();

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

            let interaction = message
                .await_component_interaction(ctx.serenity_context().shard.clone())
                .timeout(Duration::from_secs(60 * 2));

            match interaction.await {
                Some(interaction) => {
                    interaction.defer(ctx.http()).await?;
                    message.delete(ctx.http()).await?;

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

                    player.add_track_to_queue(ctx, track).await?;
                    channel_service::join_user_channel(ctx).await?;
                },
                None => {
                    message.delete(ctx.http()).await?;
                    
                    PlayerEmbed::SearchExpired
                        .to_embed()
                        .send_context(ctx, true, Some(30))
                        .await?;
                    
                    return Ok(());
                }
            };
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

            player.add_playlist_to_queue(ctx, playlist).await?;
            channel_service::join_user_channel(ctx).await?;
        }

        Err(error) => {
            return Err(MusicBotError::from(error));
        }
    }

    Ok(())
}