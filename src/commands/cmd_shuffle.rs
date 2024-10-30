use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::{
    check_author_in_same_voice_channel,
    check_if_player_is_playing,
    check_if_queue_is_not_empty
};
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_player_is_playing",
    check = "check_if_queue_is_not_empty",
)]
pub async fn shuffle(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    player.shuffle().await?;

    let embed: CreateEmbed = embed_service::create_shuffle_song_embed();
    let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;

    Ok(())
}
