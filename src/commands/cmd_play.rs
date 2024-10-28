use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service;
use crate::sources::youtube::youtube_client::{SearchError, SearchResult, YoutubeClient};
use serenity::all::{CreateEmbed, ReactionType};
use std::convert::Into;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::RwLockWriteGuard;

const YOUTUBE_VIDEO_URL: &str = "https://www.youtube.com/watch?v=";
const YOUTUBE_PLAYLIST_URL: &str = "https://www.youtube.com/playlist?list=";

const SPOTIFY_TRACK_URL: &str = "https://open.spotify.com/track/";
const SPOTIFY_PLAYLIST_URL: &str = "https://open.spotify.com/playlist/";

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn play(ctx: Context<'_>, track_source: Vec<String>) -> Result<(), MusicBotError> {
    let track_source: String = track_source.join(" ");
    let mut join_channel: bool = false;
    
    let mut result: Result<SearchResult, SearchError> = Err(SearchError::InternalError("No search result found".into()));

    // Search YouTube
    if track_source.starts_with(&YOUTUBE_VIDEO_URL)  || track_source.starts_with(&YOUTUBE_PLAYLIST_URL) {
        let youtube_client: &YoutubeClient = &ctx.data().youtube_client;

        if track_source.starts_with(&YOUTUBE_VIDEO_URL) {
            result = youtube_client.search_track_url(track_source, 1).await;
        }
        else if track_source.starts_with(&YOUTUBE_PLAYLIST_URL) {
            result = youtube_client.search_playlist_url(track_source).await;
        }
    }
    // Search Spotify
    else if track_source.starts_with(&SPOTIFY_TRACK_URL) || track_source.starts_with(&SPOTIFY_PLAYLIST_URL) {
        // TODO: Implement search for tracks using Spotify
    }
    // Search using text on YouTube
    else {
        let youtube_client: &YoutubeClient = &ctx.data().youtube_client;
        result = youtube_client.search_track_url(track_source, 6).await;
    }

    match result {
        Ok(SearchResult::Track(track)) => {
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

            if let Err(error) = player.add_track_to_queue(ctx, track.clone()).await {
                println!("Error adding track to queue: {:?}", error);
                
                let embed: CreateEmbed = embed_service::create_playback_error_embed(error);
                let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
            } else {
                join_channel = true;
            }

            drop(player);
        }

        Ok(SearchResult::Tracks(tracks)) => {
            let embed: CreateEmbed = embed_service::create_track_choose_embed(&tracks);
            let message = embed_service::send_context_embed(ctx, embed, true, None).await?;
            
            let emojis = ["1️⃣", "2️⃣", "3️⃣", "4️⃣", "5️⃣", "6️⃣", "7️⃣", "8️⃣", "9️⃣", "🔟"];
            for i in 0..tracks.len() {
                let emoji = ReactionType::from_str(emojis[i]).unwrap();
                message.react(ctx.http(), emoji.clone()).await?;
                println!("Reacted with: {:?}", emoji);
            }
            
            println!("Waiting for reaction...");
            let interaction = message
                .await_reaction(ctx)
                .timeout(Duration::from_secs(60 * 3));
            
            let track: Track = match interaction.await {
                Some(reaction) => {
                    message.delete(ctx.http()).await?;
                    
                    let selected_emoji: String = reaction.emoji.to_string();
                    let index: usize = emojis.iter().position(|&x| x == selected_emoji).unwrap();
                    
                    tracks.get(index).unwrap().clone()
                },
                None => {
                    message.delete(ctx.http()).await?;
                    
                    let embed: CreateEmbed = embed_service::create_track_choose_expired_embed();
                    let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
                    return Ok(());
                }
            };
            
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
            
            if let Err(error) = player.add_track_to_queue(ctx, track.clone()).await {
                println!("Error adding track to queue: {:?}", error);
                
                let embed: CreateEmbed = embed_service::create_playback_error_embed(error);
                let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
            }
            
            drop(player);
        }

        Ok(SearchResult::Playlist(playlist)) => {
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

            if let Err(error) = player.add_tracks_to_queue(ctx, playlist).await {
                println!("Error adding track to queue: {:?}", error);

                let embed: CreateEmbed = embed_service::create_playback_error_embed(error);
                let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
            } else {
                join_channel = true;
            }

            drop(player);
        }

        Err(error) => {
            println!("Error searching for video: {:?}", error);
            
            let embed: CreateEmbed = embed_service::create_search_error_embed(error);
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        }
    }
    
    if join_channel {
        if let Err(error) = channel_service::join_user_channel(ctx).await {
            let embed: CreateEmbed = embed_service::create_error_embed(error);
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        }
    }

    Ok(())
}