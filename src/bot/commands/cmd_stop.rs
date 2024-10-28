use crate::bot::handlers::message_handler;
use crate::bot::{
    checks::channel_checks::check_author_in_same_voice_channel,
    client::{Context, MusicBotError},
    player::playback::Playback
};
use poise::CreateReply;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn stop(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let mut playback: RwLockWriteGuard<Playback> = ctx.data().playback.write().await;

    if let Err(error) = playback.stop_playback(ctx).await {
        println!("Error stopping playback: {:?}", error);
        ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
    }
    
    drop(playback);
    Ok(())
}
