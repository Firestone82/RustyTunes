use crate::bot::{Context, MusicBotError};
use crate::handlers::error_handler::ErrorHandler;
use serenity::all::{ChannelId, GuildId, UserId};
use songbird::{Call, Event, Songbird};
use std::sync::Arc;
use tokio::sync::MutexGuard;

pub async fn join_user_channel(ctx: Context<'_>) -> Result<ChannelId, MusicBotError> {
    let guild_id: GuildId = ctx.guild_id().ok_or_else(|| {
        tracing::error!("Could not locate voice channel: guild ID is none");
        MusicBotError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
    })?;

    let chanel_id: ChannelId = match get_user_voice_channel(ctx, &ctx.author().id) {
        Some(user_channel) => user_channel,
        None => {
            tracing::debug!("User not in voice channel");
            return Err(MusicBotError::UserNotInVoiceChannelError)
        }
    };
    
    let manager: Arc<Songbird> = songbird::get(ctx.serenity_context()).await
        .ok_or_else(|| MusicBotError::InternalError("Could not locate voice channel. Songbird manager does not exist".to_owned()))?;

    match manager.join(guild_id, chanel_id).await {
        Ok(handle_lock) => {
            let mut handle: MutexGuard<Call> = handle_lock.lock().await;

            // Inactivity / disconnect handling lives in bot.rs's VoiceStateUpdate
            // listener — songbird's CoreEvent::DriverDisconnect also fires on
            // transient drops (e.g. when an admin moves the bot), which is too
            // aggressive for a "leave the channel" trigger.
            handle.add_global_event(
                Event::Track(songbird::TrackEvent::Error),
                ErrorHandler
            );
        }

        Err(error) => {
            tracing::error!("Error joining voice channel: {:?}", error);
            return Err(MusicBotError::UnableToJoinVoiceChannelError)
        }
    }

    Ok(chanel_id)
}

pub async fn leave_channel(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let guild_id: GuildId = ctx.guild_id().ok_or_else(|| {
        tracing::error!("Could not locate voice channel: guild ID is none");
        MusicBotError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
    })?;

    let manager: Arc<Songbird> = songbird::get(ctx.serenity_context()).await
        .ok_or_else(|| MusicBotError::InternalError("Songbird manager not registered".to_owned()))?;

    // Always stop playback regardless of voice state.
    let _ = ctx.data().player.write().await.stop_playback().await;

    // If songbird is tracking a Call, leave properly. Otherwise it's already
    // gone (e.g. the alone-in-channel handler removed it) and there's nothing
    // to do — treat it as success rather than error.
    if let Some(handle_lock) = manager.get(guild_id) {
        let mut handle: MutexGuard<Call> = handle_lock.lock().await;
        handle.remove_all_global_events();
        if let Err(error) = handle.leave().await {
            tracing::error!("Could not leave voice channel: {:?}", error);
            return Err(MusicBotError::InternalError("Could not leave voice channel".to_owned()));
        }
    } else {
        tracing::debug!("/leave called but no active songbird call; treating as no-op");
    }

    Ok(())
}

pub fn get_user_voice_channel(ctx: Context<'_>, user_id: &UserId) -> Option<ChannelId> {
    ctx
        .guild()
        .as_ref()
        .and_then(|guild| guild.voice_states.get(user_id))
        .and_then(|voice_state| voice_state.channel_id)
}