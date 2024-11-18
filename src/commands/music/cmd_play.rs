use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use crate::sources::youtube::youtube_client::{SearchError, YouTubeSearchResult, YoutubeClient};
use serenity::all::{Message, ReactionType};
use std::convert::Into;
use std::str::FromStr;
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
        result = youtube_client.search_track_url(track_source, 6).await;
    }

    match result {
        Ok(YouTubeSearchResult::Track(track)) => {
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

            QueueEmbed::TrackAdded(&track)
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;

            player.add_track_to_queue(ctx, track.clone()).await?;
            channel_service::join_user_channel(ctx).await?;
        }

        Ok(YouTubeSearchResult::Tracks(mut tracks)) => {
            let message: Message = PlayerEmbed::Search(&tracks)
                .to_embed()
                .send_context(ctx, true, None)
                .await?;
            
            let emojis = ["1️⃣", "2️⃣", "3️⃣", "4️⃣", "5️⃣", "6️⃣", "7️⃣", "8️⃣", "9️⃣", "🔟"];
            for i in 0..tracks.len() {
                let emoji: ReactionType = ReactionType::from_str(emojis[i]).unwrap();
                message.react(ctx.http(), emoji.clone()).await?;
            }
            
            let interaction = message
                .await_reaction(ctx)
                .timeout(Duration::from_secs(60 * 2));
            
            match interaction.await {
                Some(reaction) => {
                    message.delete(ctx.http()).await?;
                    
                    let track_index: usize = emojis.iter().position(|&x| x == reaction.emoji.to_string()).unwrap();
                    let track: Track = tracks.swap_remove(track_index);

                    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

                    if !player.queue.is_empty() {
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

        Ok(YouTubeSearchResult::Playlist(playlist)) => {
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