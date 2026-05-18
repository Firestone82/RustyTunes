use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::checks::player_checks::check_if_player_is_playing;
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::RwLockWriteGuard;

/// Stop the current playback.
#[poise::command(
    prefix_command,
    slash_command,
    check = "check_author_in_same_voice_channel",
    check = "check_if_player_is_playing"
)]
pub async fn stop(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    // Reset the flag so the freshly spawned inactivity timer isn't immediately
    // cancelled (push_track sets it to true whenever a new track is queued).
    player.inactivity_cancel.store(false, Ordering::Relaxed);
    let cancel = Arc::clone(&player.inactivity_cancel);

    player.stop_playback().await?;
    crate::player::player::set_idle(ctx.serenity_context());
    drop(player);

    PlayerEmbed::Stopped
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;

    // QueueHandler only spawns the inactivity timer when the queue empties
    // naturally (TrackEnd event). When the user calls !stop the track is
    // stopped programmatically, so QueueHandler sees is_playing=false and
    // exits early without starting the timer. Spawn it here instead.
    let guild_id = ctx
        .guild_id()
        .ok_or_else(|| MusicBotError::InternalError("no guild".into()))?;
    let Some(guild_channel) = ctx.guild_channel().await else {
        return Ok(());
    };
    let serenity_ctx = ctx.serenity_context().clone();
    let player_arc = ctx.data().player.clone();

    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(5 * 60)).await;

        if cancel.load(Ordering::Relaxed) {
            tracing::debug!("Inactivity timer cancelled after stop — new track was queued");
            return;
        }
        if player_arc.read().await.is_playing {
            return;
        }

        tracing::info!("Leaving voice channel after 5 minutes of inactivity following stop");

        let _ = PlayerEmbed::InactivityLeave
            .to_embed()
            .send_channel(serenity_ctx.http.clone(), &guild_channel, Some(60), None)
            .await;

        let _ = player_arc.write().await.stop_playback().await;

        if let Some(manager) = songbird::get(&serenity_ctx).await {
            let _ = manager.remove(guild_id).await;
        }
    });

    Ok(())
}
