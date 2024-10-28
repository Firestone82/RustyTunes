use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
)]
pub async fn shuffle(ctx: Context<'_>, page: Option<usize>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
    
    match player.shuffle().await {
        Ok(_) => {
            let embed: CreateEmbed = embed_service::create_shuffle_song_embed();
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        },

        Err(error) => {
            println!("Error shuffling queue: {:?}", error);

            let embed: CreateEmbed = embed_service::create_playback_error_embed(error);
            let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
        }
    }

    drop(player);
    Ok(())
}
