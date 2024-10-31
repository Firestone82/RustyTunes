use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;
use crate::checks::player_checks::{check_if_player_is_playing, check_if_queue_is_not_empty};

#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_player_is_playing",
    check = "check_if_queue_is_not_empty",
)]
pub async fn shuffle(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    player.shuffle().await?;

    PlayerEmbed::Shuffled
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;

    Ok(())
}
