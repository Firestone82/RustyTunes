use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::queue_embed::QueueEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;
use crate::checks::player_checks::{check_if_player_is_playing, check_if_queue_is_not_empty};

#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_player_is_playing",
    check = "check_if_queue_is_not_empty"
)]
pub async fn skip(ctx: Context<'_>, amount: Option<usize>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    let amount: usize = player.skip(amount.unwrap_or(1)).await?;
    
    QueueEmbed::Skipped(amount)
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;

    drop(player);
    Ok(())
}
