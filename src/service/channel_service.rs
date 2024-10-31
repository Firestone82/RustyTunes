use crate::bot::{Context, MusicBotError};
use crate::handlers::disconnect_handler::DisconnectHandler;
use crate::handlers::error_handler::ErrorHandler;
use crate::handlers::inactivity_handler::InactivityHandler;
use serenity::all::{ChannelId, GuildId, UserId};
use songbird::{Call, CoreEvent, Event, Songbird};
use std::sync::Arc;
use tokio::sync::MutexGuard;

pub async fn join_user_channel(ctx: Context<'_>) -> Result<ChannelId, MusicBotError> {
    let guild_id: GuildId = ctx.guild_id().ok_or_else(|| {
        println!("Could not locate voice channel. Guild ID is none");
        MusicBotError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
    })?;

    let chanel_id: ChannelId = match get_user_voice_channel(ctx, &ctx.author().id) {
        Some(user_channel) => user_channel,
        None => {
            println!("User not in voice channel");
            return Err(MusicBotError::UserNotInVoiceChannelError)
        }
    };
    
    let manager: Arc<Songbird> = songbird::get(ctx.serenity_context()).await
        .ok_or_else(|| MusicBotError::InternalError("Could not locate voice channel. Songbird manager does not exist".to_owned()))?;

    match manager.join(guild_id, chanel_id).await {
        Ok(handle_lock) => {
            let mut handle: MutexGuard<Call> = handle_lock.lock().await;

            // Event listener to disconnect the bot if the driver disconnects
            handle.add_global_event(
                Event::Core(CoreEvent::DriverDisconnect),
                DisconnectHandler::new(guild_id, manager.clone(), ctx.data().player.clone()),
            );

            // Event listener to disconnect the bot if there is no activity in the voice channel
            handle.add_global_event(
                Event::Core(CoreEvent::ClientDisconnect),
                InactivityHandler::new(guild_id, manager.clone(), ctx.serenity_context().clone())
            );

            // Event listener for when there is an error with the track
            handle.add_global_event(
                Event::Track(songbird::TrackEvent::Error),
                ErrorHandler
            );
        }

        Err(error) => {
            println!("Error joining voice channel: {:?}", error);
            return Err(MusicBotError::UnableToJoinVoiceChannelError)
        }
    }

    Ok(chanel_id)
}

pub async fn leave_channel(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let guild_id: GuildId = ctx.guild_id().ok_or_else(|| {
        println!("Could not locate voice channel. Guild ID is none");
        MusicBotError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
    })?;

    let manager: Arc<Songbird> = songbird::get(ctx.serenity_context()).await
        .ok_or_else(|| MusicBotError::InternalError("Could not locate voice channel. Songbird manager does not exist".to_owned()))?;

    match manager.get(guild_id) {
        Some(handle_lock) => {
            let mut handle: MutexGuard<Call> = handle_lock.lock().await;

            handle.remove_all_global_events();
            handle.leave().await
                .map_err(|error| {
                    println!("Could not leave voice channel. Error: {:?}", error);
                    MusicBotError::InternalError("Could not leave voice channel".to_owned())
                }).expect("Could not leave voice channel");
        }

        None => {
            println!("Could not locate voice channel. Songbird manager does not exist");
            return Err(MusicBotError::InternalError("Could not locate voice channel. Songbird manager does not exist".to_owned()))
        }
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