use crate::bot::{
    checks::channel_checks::check_author_in_same_voice_channel,
    client::{Context, MusicBotError},
    handlers::{channel_handler, message_handler},
    player::playback::{Playback, Track},
    youtube::client::{YoutubeClient, YoutubeError}
};
use poise::CreateReply;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn play(ctx: Context<'_>, youtube_url: Vec<String>) -> Result<(), MusicBotError> {
    let youtube_url: String = youtube_url.join(" ");
    
    if let Err(error) = channel_handler::join_user_channel(ctx).await {
        println!("Error joining voice channel: {:?}", error);
        ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
    }

    let youtube_client: &YoutubeClient = &ctx.data().youtube_client;
    let result: Result<Track, YoutubeError> = youtube_client.fetch_track(youtube_url).await;

    match result {
        Ok(track) => {
            let mut playback: RwLockWriteGuard<Playback> = ctx.data().playback.write().await;
            
            if let Err(error) = playback.add_track_to_queue(ctx, track).await {
                println!("Error adding track to queue: {:?}", error);
                ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
            }
            
            drop(playback);
        }

        Err(error) => {
            println!("Error searching for video: {:?}", error);
            ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
        }
    }

    Ok(())
}
