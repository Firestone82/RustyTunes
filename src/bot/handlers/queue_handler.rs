use crate::bot::handlers::message_handler;
use crate::bot::player::playback::Playback;
use async_trait::async_trait;
use lombok::AllArgsConstructor;
use poise::serenity_prelude;
use serenity::all::GuildChannel;
use songbird::{
    input::YoutubeDl,
    tracks::TrackHandle,
    {Call, Event, EventContext, EventHandler}
};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard, RwLock};

#[derive(AllArgsConstructor, Clone)]
pub struct QueueHandler {
    serenity_ctx: serenity_prelude::Context,
    manager: Arc<Mutex<Call>>,
    req_client: reqwest::Client,
    playback: Arc<RwLock<Playback>>,
    guild_channel: GuildChannel,
}

#[async_trait]
impl EventHandler for QueueHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        let mut playback = self.playback.write().await;
        
        if !playback.is_playing {
            return None;
        }
        
        println!("Track has ended. Requesting next song to play.");
        
        match playback.queue.pop() {
            Some(next_track) => {
                println!("- Playing next track: {}", next_track.metadata.title);

                // Send "Now playing message"
                let _ = self.guild_channel.send_message(
                    self.serenity_ctx.http.clone(),
                    serenity_prelude::CreateMessage::default()
                        .embed( message_handler::create_now_playing_embed(&next_track))
                ).await;

                // Play the next track
                let mut guard: MutexGuard<Call> = self.manager.lock().await;
                let track: YoutubeDl = YoutubeDl::new(self.req_client.clone(), next_track.metadata.track_url.clone());
                let track_handle: TrackHandle = guard.play(track.into());

                // Add event to handle the track end
                let _ = track_handle
                    .add_event(
                        Event::Track(songbird::TrackEvent::End),
                        self.clone()
                    )
                    .map_err(|e| {
                        println!("Error adding event to track handle: {:?}", e);
                    });

                playback.track_handle = Some(track_handle);
            }

            None => {
                println!("- No more tracks to play. Stopping playback.");
                playback.is_playing = false;
                playback.current_track = None;
                playback.track_handle = None;
            }
        }

        None
    }
}