use crate::bot::{MusicBotData, MusicBotError};
use crate::player::player;
use serenity::all::{ChannelId, FullEvent};
use serenity::prelude::Context as SerenityContext;

/// Handle `VoiceStateUpdate` events: auto-arrive expected gathering users
/// and clean up playback when the bot is left alone or removed from voice.
pub async fn handle(
    ctx: &SerenityContext,
    event: &FullEvent,
    data: &MusicBotData,
) -> Result<(), MusicBotError> {
    let FullEvent::VoiceStateUpdate { new, .. } = event else {
        return Ok(());
    };

    let guild_id = match new.guild_id {
        Some(g) => g,
        None => return Ok(()),
    };

    if let Some(joined_channel) = new.channel_id {
        let gatherings = data.gatherings.read().await;
        if let Some(gather_state) = gatherings.get(&guild_id) {
            if gather_state.voice_channel_id == joined_channel {
                let is_expected = gather_state
                    .extra_expected
                    .lock()
                    .unwrap()
                    .contains(&new.user_id);
                // A user who opted out and was disconnected can take back the
                // opt-out by rejoining — voice_handler routes both flows
                // through `auto_arrived` and the check-in loop tells them
                // apart by consulting `reconsidering`.
                let is_reconsidering = gather_state
                    .reconsidering
                    .lock()
                    .unwrap()
                    .contains(&new.user_id);
                if is_expected || is_reconsidering {
                    gather_state
                        .auto_arrived
                        .lock()
                        .unwrap()
                        .insert(new.user_id);
                }
            }
        }
    }

    let bot_id = ctx.cache.current_user().id;

    let bot_channel: Option<ChannelId> = ctx
        .cache
        .guild(guild_id)
        .as_ref()
        .and_then(|g| g.voice_states.get(&bot_id))
        .and_then(|vs| vs.channel_id);

    // Bot is no longer in voice (kicked, dragged out, force-disconnected).
    // A paused track still holds queue state, so always wipe both.
    if bot_channel.is_none() {
        let mut player = data.player.write().await;
        let needs_cleanup = player.is_playing || player.is_paused || !player.queue.is_empty();

        if needs_cleanup {
            tracing::info!("Bot is no longer in a voice channel. Cleaning up playback state.");
            let _ = player.stop_playback().await;
            drop(player);
            player::set_idle(ctx);

            if let Some(manager) = songbird::get(ctx).await {
                let _ = manager.remove(guild_id).await;
            }
        }
        return Ok(());
    }

    let bot_channel = bot_channel.unwrap();

    let humans = ctx
        .cache
        .guild(guild_id)
        .as_ref()
        .map(|g| {
            g.voice_states
                .values()
                .filter(|vs| vs.channel_id == Some(bot_channel) && vs.user_id != bot_id)
                .count()
        })
        .unwrap_or(0);

    if humans == 0 {
        tracing::info!("Bot is alone in voice channel. Leaving.");

        let _ = data.player.write().await.stop_playback().await;
        player::set_idle(ctx);

        if let Some(manager) = songbird::get(ctx).await {
            let _ = manager.remove(guild_id).await;
        }
    }

    Ok(())
}
