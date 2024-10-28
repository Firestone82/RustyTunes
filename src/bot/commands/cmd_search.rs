use crate::bot::{
    checks::channel_checks::check_author_in_same_voice_channel,
    client::{Context, MusicBotError},
    handlers::{channel_handler, message_handler},
    player::playback::{Playback, Track},
    youtube::client::{YoutubeClient, YoutubeError}
};
use poise::CreateReply;
use serenity::all::{CreateEmbed, ReactionType};
use std::{
    str::FromStr,
    time::Duration
};
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn search(ctx: Context<'_>, youtube_url: Vec<String>) -> Result<(), MusicBotError> {
    let youtube_url: String = youtube_url.join(" ");
    
    if let Err(error) = channel_handler::join_user_channel(ctx).await {
        println!("Error joining voice channel: {:?}", error);
        ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
    }

    let youtube_client: &YoutubeClient = &ctx.data().youtube_client;
    let result: Result<Vec<Track>, YoutubeError> = youtube_client.search_track(youtube_url).await;

    match result {
        Ok(tracks) => {
            let mut embed = CreateEmbed::new()
                .title("🔍 Search results")
                .description("Select a track to play by clicking on the corresponding reaction.");

            let mut index = 1;
            for track in tracks.clone() {
                embed = embed.field(format!(":number_{}:  {}", index, track.metadata.title), track.metadata.track_url, false);
                index += 1;
            }

            let message = ctx.send(CreateReply::default().embed(embed)).await?;
            
            match message.into_message().await {
                Ok(message) => {
                    let emojis = ["1️⃣", "2️⃣", "3️⃣", "4️⃣", "5️⃣", "6️⃣", "7️⃣", "8️⃣", "9️⃣", "🔟"];
                    
                    for i in 0..tracks.len() {
                        let emoji = ReactionType::from_str(emojis[i]).unwrap();
                        message.react(ctx.http(), emoji).await?;
                    }
                    
                    let interaction = match message
                        .await_reaction(ctx)
                        .timeout(Duration::from_secs(60 * 3))
                        .await
                    {
                        Some(x) => x,
                        None => {
                            ctx.send(CreateReply::default().content("No track selected.")).await?;
                            return Ok(());
                        },
                    };
                    
                    // Delete the message after the user has selected a track
                    message.delete(ctx.http()).await?;
                    
                    // Add selected track to queue
                    let mut playback: RwLockWriteGuard<Playback> = ctx.data().playback.write().await;
                    
                    let selected_emoji: String = interaction.emoji.to_string();
                    let index: usize = emojis.iter().position(|&x| x == selected_emoji).unwrap();

                    if let Err(error) = playback.add_track_to_queue(ctx, tracks.get(index).unwrap().clone()).await {
                        println!("Error adding track to queue: {:?}", error);
                        ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
                    }

                    drop(playback);
                },
                Err(error) => {
                    println!("Error sending message: {:?}", error);
                    return Err(MusicBotError::InternalError("Error sending message".to_string()));
                }
            };
        }

        Err(error) => {
            println!("Error searching for video: {:?}", error);
            ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
        }
    }

    Ok(())
}
