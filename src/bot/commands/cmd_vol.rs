use crate::bot::{
    checks::channel_checks::check_author_in_same_voice_channel,
    client::{Context, MusicBotError},
    handlers::message_handler,
    player::playback::Playback
};
use poise::CreateReply;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn vol(ctx: Context<'_>, volume: f32) -> Result<(), MusicBotError> {
    let mut playback: RwLockWriteGuard<Playback> = ctx.data().playback.write().await;

    if let Err(error) = playback.set_volume(ctx, volume).await {
        println!("Error stopping playback: {:?}", error);
        ctx.send(CreateReply::default().embed(message_handler::create_playback_error_embed(error.to_string()))).await?;
    }
    
    drop(playback);
    Ok(())
}
