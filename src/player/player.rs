use crate::bot::Context;
use crate::handlers::queue_handler::QueueHandler;
use crate::service::embed_service;
use serenity::all::{CreateEmbed, GuildId};
use songbird::input::YoutubeDl;
use songbird::tracks::TrackHandle;
use songbird::{Call, Event, TrackEvent};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};

#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    #[error("Whoops, an internal error occurred: {0}")]
    InternalError(String),

    #[error("No tracks in queue")]
    NoTracksInQueue,

    #[error("Playback is not active")]
    PlaybackNotActive,

    #[error("Playback is already active")]
    PlaybackAlreadyActive
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: String,
    pub metadata: TrackMetadata
}

#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub title: String,
    pub channel: String,
    pub track_url: String,
    pub thumbnail_url: String,
}

pub struct Player {
    pub is_playing: bool,
    pub track_handle: Option<TrackHandle>,
    pub current_track: Option<Track>,
    pub queue: Vec<Track>,
    pub volume: f32
}

impl Default for Player {
    fn default() -> Self {
        Player {
            is_playing: false,
            track_handle: None,
            current_track: None,
            queue: Vec::new(),
            volume: 0.5
        }
    }
}

impl Player {
    pub async fn add_track_to_queue(&mut self, ctx: Context<'_>, track: Track) -> Result<(), PlaybackError> {
        println!("Adding track to queue: {}", track.metadata.track_url);

        self.queue.push(track.clone());
        println!("- Queue length: {}", self.queue.len());

        let embed: CreateEmbed = embed_service::create_track_added_to_queue(&self.queue, &track);
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await
            .map_err(|error| {
                println!("Error sending track added to queue embed: {:?}", error);
                PlaybackError::InternalError("Error sending track added to queue embed".to_owned())
            })?;

        if !self.is_playing {
            self.start_playback(ctx).await?;
        }

        Ok(())
    }

    pub async fn skip(&mut self, mut amount: usize) -> Result<usize, PlaybackError> {
        println!("Skipping {} track(s)", amount);

        if !self.is_playing {
            println!("- Playback is not active");
            return Err(PlaybackError::PlaybackNotActive);
        }

        if amount > self.queue.len() {
            println!("- Amount to skip is greater than queue length. Skipping all tracks");
            amount = amount.min(self.queue.len());
        }

        if self.queue.is_empty() && self.is_playing {
            println!("- No tracks in queue. Stopping playback");
            self.stop_playback().await?;

            return Ok(1);
        }

        if !self.queue.is_empty() {
            if amount > 1 {
                self.queue.drain(0..amount-1);
            }

            self.stop_track().await?;
            self.is_playing = true;
        }

        Ok(amount)
    }

    pub async fn start_playback(&mut self, ctx: Context<'_>) -> Result<(), PlaybackError> {
        if self.is_playing {
            return Err(PlaybackError::PlaybackAlreadyActive);
        }

        if self.queue.is_empty() {
            return Err(PlaybackError::NoTracksInQueue);
        }

        let _ = self.next_track(ctx).await?;
        Ok(())
    }

    pub async fn next_track(&mut self, ctx: Context<'_>) -> Result<Option<&Track>, PlaybackError> {
        println!("Requesting next track to play");

        let guild_id: GuildId = ctx.guild_id()
            .ok_or_else(|| {
                println!("Could not locate voice channel. Guild ID is none");
                PlaybackError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
            })?;

        let manager: Arc<Mutex<Call>> = songbird::get(ctx.serenity_context()).await
            .ok_or_else(|| {
                println!("Could not locate voice channel. Guild ID is none");
                PlaybackError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
            })?
            .get_or_insert(guild_id);

        if self.is_playing {
            self.stop_track().await?;
        }

        match self.queue.pop() {
            Some(next_track) => {
                println!("- Found: {}", next_track.metadata.title);

                // Send "Now playing message"
                let embed: CreateEmbed = embed_service::create_now_playing_embed(&next_track);
                let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await
                    .map_err(|error| {
                        println!("Error sending now playing embed: {:?}", error);
                        PlaybackError::InternalError("Error sending now playing embed".to_owned())
                    })?;

                // Play the next track
                let mut guard: MutexGuard<Call> = manager
                    .lock()
                    .await;

                let track: YoutubeDl = YoutubeDl::new(ctx.data().request_client.clone(), next_track.metadata.track_url.clone());
                let track_handle: TrackHandle = guard.play(track.into());
                
                // Set volume
                let _ = track_handle.set_volume(self.volume);

                // Add event to handle the track end
                let _ = track_handle.add_event(
                    Event::Track(TrackEvent::End),
                    QueueHandler::new(
                        ctx.serenity_context().clone(),
                        manager.clone(),
                        ctx.data().request_client.clone(),
                        ctx.data().player.clone(),
                        ctx.guild_channel().await.unwrap()
                    )
                )
                .map_err(|e| {
                    println!("Error adding event to track handle: {:?}", e);
                });

                self.current_track = Some(next_track);
                self.track_handle = Some(track_handle);
                self.is_playing = true;

                Ok(self.current_track.as_ref())
            }

            None => {
                println!("- No more tracks to play. Stopping playback.");
                self.stop_playback().await?;
                Ok(None)
            }
        }
    }

    pub async fn set_volume(&mut self, mut volume: f32) -> Result<(), PlaybackError> {
        volume = volume / 20.0;
        
        if let Some(track_handle) = &self.track_handle {
            let _ = track_handle.set_volume(volume);
        }

        self.volume = volume;
        Ok(())
    }

    pub async fn stop_track(&mut self) -> Result<(), PlaybackError> {
        if self.is_playing {
            println!("Stopping track");

            if let Some(track_handle) = &self.track_handle {
                if let Err(error) = track_handle.stop() {
                    println!("- Error stopping track: {:?}", error);
                    return Err(PlaybackError::InternalError(format!("Error stopping track: {:?}", error)));
                }
            }
        }

        self.is_playing = false;
        self.track_handle = None;
        self.current_track = None;

        Ok(())
    }

    pub async fn stop_playback(&mut self) -> Result<(), PlaybackError> {
        self.stop_track().await?;
        self.queue.clear();

        Ok(())
    }

}