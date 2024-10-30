use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::player::player::{Player, Track};
use crate::service::channel_service;
use crate::service::embed_service;
use crate::sources::youtube::youtube_client::{SearchError, SearchResult, YoutubeClient};
use serenity::all::{CreateEmbed, Message, ReactionType};
use std::convert::Into;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::RwLockWriteGuard;
use crate::commands::cmd_playing::playing;

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
    
    let mut result: Result<SearchResult, SearchError> = Err(SearchError::InternalError("No search result found".into()));

    // Search YouTube
    if track_source.starts_with(&YOUTUBE_VIDEO_URL) || track_source.starts_with(&YOUTUBE_PLAYLIST_URL) {
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

            player.add_track_to_queue(ctx, track.clone()).await?;
            channel_service::join_user_channel(ctx).await?;
            
            let embed: CreateEmbed = embed_service::create_track_added_to_queue(&player.queue, &track);
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        }

        Ok(SearchResult::Tracks(mut tracks)) => {
            let embed: CreateEmbed = embed_service::create_track_choose_embed(&tracks);
            let message: Message = embed_service::send_context_embed(ctx, embed, true, None).await?;
            
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
                    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
                    
                    message.delete(ctx.http()).await?;
                    
                    let selected_emoji: String = reaction.emoji.to_string();
                    let track_index: usize = emojis.iter().position(|&x| x == selected_emoji).unwrap();
                    
                    // TODO: Question: Why do I need to clone the track here? If its already in the tracks vector?
                    // let track: Track = tracks.get(track_index).unwrap().clone();
                    let track: Track = tracks.remove(track_index);
                    
                    player.add_track_to_queue(ctx, track.clone()).await?;
                    channel_service::join_user_channel(ctx).await?;
                    
                    let embed: CreateEmbed = embed_service::create_track_added_to_queue(&player.queue, &track);
                    let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
                },
                None => {
                    message.delete(ctx.http()).await?;
                    
                    let embed: CreateEmbed = embed_service::create_track_choose_expired_embed();
                    let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
                    
                    return Ok(());
                }
            };
        }

        Ok(SearchResult::Playlist(playlist)) => {
            let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

            player.add_tracks_to_queue(ctx, playlist).await?;
            channel_service::join_user_channel(ctx).await?;
        }

        Err(error) => {
            println!("Error searching for video: {:?}", error);
            
            let embed: CreateEmbed = embed_service::create_search_error_embed(error);
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        }
    }

    Ok(())
}