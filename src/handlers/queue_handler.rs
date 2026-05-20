use crate::embeds::music::player_embed::PlayerEmbed;
use crate::player::player::{self, Player};
// Odebral jsem PlaybackError, v tomto kontextu nebyl správně použit
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use async_trait::async_trait;
use lombok::AllArgsConstructor;
use poise::serenity_prelude;
use serenity::all::{GuildChannel, GuildId};
use songbird::{Call, Event, EventContext, EventHandler};
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(AllArgsConstructor, Clone)]
pub struct QueueHandler {
    serenity_ctx: serenity_prelude::Context,
    manager: Arc<tokio::sync::Mutex<Call>>,
    req_client: reqwest::Client,
    player: Arc<tokio::sync::RwLock<Player>>,
    guild_channel: GuildChannel,
    guild_id: GuildId,
}

#[async_trait]
impl EventHandler for QueueHandler {
    async fn act(
        &self,
        _e: &EventContext<'_>,
    ) -> Option<Event> {
        let mut player = self.player.write().await;

        if !player.is_playing {
            return None;
        }

        tracing::info!("Track ended; advancing queue");

        if player.queue.is_empty() {
            tracing::info!("No more tracks to play. Stopping playback.");
            player::set_idle(&self.serenity_ctx);

            player.track_handle = None;
            player.current_track = None;
            player.is_playing = false;

            player.inactivity_cancel.store(false, Ordering::Relaxed);

            let cancel = Arc::clone(&player.inactivity_cancel);
            let serenity_ctx = self.serenity_ctx.clone();
            let player_arc = self.player.clone();
            let guild_id = self.guild_id;

            drop(player);

            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(5 * 60)).await;

                if cancel.load(Ordering::Relaxed) {
                    tracing::debug!("Inactivity timer cancelled - new track was queued");
                    return;
                }

                if player_arc.read().await.is_playing {
                    return;
                }

                // Voice handler already cleaned up if the bot was kicked or
                // dragged out — don't announce a leave we didn't perform.
                let Some(voice_channel_id) = channel_service::bot_voice_channel(&serenity_ctx, guild_id) else {
                    tracing::debug!("Bot already left voice channel — skipping inactivity leave notice");
                    return;
                };

                tracing::info!("Leaving voice channel after 5 minutes of inactivity");

                let _ = PlayerEmbed::InactivityLeave
                    .to_embed()
                    .send_channel_id(serenity_ctx.http.clone(), voice_channel_id, Some(60), None)
                    .await;

                let _ = player_arc.write().await.stop_playback().await;

                if let Some(manager) = songbird::get(&serenity_ctx).await {
                    let _ = manager.remove(guild_id).await;
                }
            });

            return None;
        }

        let next_track = player.queue.remove(0);

        tracing::info!("Playing next track: {}", next_track.metadata.title);

        if !player.silent {
            if let Err(e) = PlayerEmbed::NowPlaying(&next_track)
                .to_embed()
                .send_channel(
                    self.serenity_ctx.http.clone(),
                    &self.guild_channel,
                    Some(30),
                    None,
                )
                .await
            {
                tracing::error!("Error sending now playing embed: {e:?}");
            }
        }

        let (input, source_path) = next_track.resolve_input(&self.req_client).await;

        let track_handle = self.manager.lock().await.play(input.into());

        player.current_gain = 1.0;
        player.current_source_path = source_path.clone();
        let _ = track_handle.set_volume(player.volume);

        if let Some(path) = source_path {
            if player.should_normalize() {
                player::schedule_normalization_apply(
                    self.player.clone(),
                    track_handle.clone(),
                    path,
                    next_track.id.clone(),
                );
            }
        } else {
            player::spawn_cache_and_apply(
                next_track.clone(),
                self.player.clone(),
                track_handle.clone(),
            );
        }

        let _ = track_handle.add_event(Event::Track(songbird::TrackEvent::End), self.clone());

        player::set_now_playing(&self.serenity_ctx, &next_track);

        player.push_to_history(next_track.clone());
        player.track_handle = Some(track_handle);
        player.current_track = Some(next_track);
        player.is_playing = true;

        None
    }
}
