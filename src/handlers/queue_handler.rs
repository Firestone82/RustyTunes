use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::{self, PlaybackError, Player};
use crate::service::embed_service::SendEmbed;
use async_trait::async_trait;
use lombok::AllArgsConstructor;
use poise::serenity_prelude;
use serenity::all::{GuildChannel, GuildId};
use songbird::{
    tracks::TrackHandle,
    {Call, Event, EventContext, EventHandler}
};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::{Mutex, MutexGuard, RwLock, RwLockWriteGuard};

#[derive(AllArgsConstructor, Clone)]
pub struct QueueHandler {
    serenity_ctx: serenity_prelude::Context,
    manager: Arc<Mutex<Call>>,
    req_client: reqwest::Client,
    player: Arc<RwLock<Player>>,
    guild_channel: GuildChannel,
    guild_id: GuildId,
}

#[async_trait]
impl EventHandler for QueueHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        let mut player: RwLockWriteGuard<Player> = self.player.write().await;

        if !player.is_playing {
            return None;
        }

        tracing::info!("Track ended; advancing queue");

        let next = if player.queue.is_empty() { None } else { Some(player.queue.remove(0)) };
        match next {
            Some(next_track) => {
                tracing::info!("Playing next track: {}", next_track.metadata.title);

                // Send "Now playing message" unless the guild has session-only silent mode on.
                if !player.silent {
                    let _ = PlayerEmbed::NowPlaying(&next_track)
                        .to_embed()
                        .send_channel(self.serenity_ctx.http.clone(), &self.guild_channel, Some(30), None)
                        .await
                        .map_err(|error| {
                            tracing::error!("Error sending now playing embed: {:?}", error);
                            PlaybackError::InternalError("Error sending now playing embed".to_owned())
                        });
                }

                let (input, source_path) = next_track
                    .resolve_input(&self.req_client)
                    .await;

                // Play the next track
                let mut guard: MutexGuard<Call> = self.manager
                    .lock()
                    .await;

                let track_handle: TrackHandle = guard.play(input.into());

                // Reset gain for the new track. Streamed inputs get cached
                // in the background and the cache helper applies the gain
                // to the live handle once ffmpeg returns — see
                // `spawn_cache_and_apply`.
                player.current_gain = 1.0;
                player.current_source_path = source_path.clone();
                let _ = track_handle.set_volume(player.volume);

                match source_path {
                    Some(path) => {
                        if player.should_normalize() {
                            player::schedule_normalization_apply(
                                self.player.clone(),
                                track_handle.clone(),
                                path,
                                next_track.id.clone(),
                            );
                        }
                    }
                    None => {
                        player::spawn_cache_and_apply(
                            next_track.clone(),
                            self.player.clone(),
                            track_handle.clone(),
                        );
                    }
                }

                // Add event to handle the track end
                let _ = track_handle.add_event(
                    Event::Track(songbird::TrackEvent::End),
                    self.clone()
                );

                player::set_now_playing(&self.serenity_ctx, &next_track);

                player.push_to_history(next_track.clone());
                player.track_handle = Some(track_handle);
                player.current_track = Some(next_track);
                player.is_playing = true;
            }

            None => {
                tracing::info!("No more tracks to play. Stopping playback.");

                player::set_idle(&self.serenity_ctx);

                player.track_handle = None;
                player.current_track = None;
                player.is_playing = false;

                // Begin 5-minute inactivity countdown
                player.inactivity_cancel.store(false, Ordering::SeqCst);
                let cancel = Arc::clone(&player.inactivity_cancel);
                let serenity_ctx = self.serenity_ctx.clone();
                let player_arc = self.player.clone();
                let guild_id = self.guild_id;
                let guild_channel = self.guild_channel.clone();

                drop(player);

                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5 * 60)).await;

                    if cancel.load(Ordering::SeqCst) {
                        tracing::debug!("Inactivity timer cancelled — new track was queued");
                        return;
                    }

                    let player = player_arc.read().await;
                    if player.is_playing {
                        return;
                    }
                    drop(player);

                    tracing::info!("Leaving voice channel after 5 minutes of inactivity");

                    let _ = PlayerEmbed::InactivityLeave
                        .to_embed()
                        .send_channel(serenity_ctx.http.clone(), &guild_channel, Some(60), None)
                        .await;

                    let mut player = player_arc.write().await;
                    let _ = player.stop_playback().await;
                    drop(player);

                    if let Some(manager) = songbird::get(&serenity_ctx).await {
                        let _ = manager.remove(guild_id).await;
                    }
                });

                return None;
            }
        }

        None
    }
}
