use crate::bot::client::Context;
use crate::bot::handlers::message_handler;
use crate::bot::handlers::queue_handler::QueueHandler;
use lombok::{AllArgsConstructor, NoArgsConstructor};
use serenity::all::{Color, GuildChannel, GuildId, Timestamp};
use serenity::builder::{CreateEmbed, CreateEmbedFooter};
use songbird::{input::YoutubeDl, tracks::TrackHandle, TrackEvent, {Call, Event}};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};

#[derive(thiserror::Error, Debug)]
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

#[derive(AllArgsConstructor, NoArgsConstructor)]
pub struct Playback {
    pub is_playing: bool,
    pub current_track: Option<Track>,
    pub track_handle: Option<TrackHandle>,
    pub queue: Vec<Track>
}

impl Playback {
    pub async fn add_track_to_queue(&mut self, ctx: Context<'_>, track: Track) -> Result<(), PlaybackError>{
        println!("Adding track to queue: {}", track.metadata.track_url);
        self.queue.push(track.clone());
        println!("- Queue length: {}", self.queue.len());
        
        let embed: CreateEmbed = CreateEmbed::new()
            .color(Color::DARK_GREEN)
            .title("🎵  Track added to queue")
            .description(format!("**[{}]({})**", track.metadata.title, track.metadata.track_url))
            .footer(CreateEmbedFooter::new(format!("Queue length: {}", self.queue.len())));
        let _ = message_handler::send_embed(&ctx, embed, true).await;
        
        if !self.is_playing {
            self.start_playback(ctx).await?;
        }

        Ok(())
    }

    pub async fn next_track(&mut self, ctx: Context<'_>) -> Result<Option<&Track>, PlaybackError> {
        println!("Requesting next track to play");

        let guild_id: GuildId = ctx.guild_id()
            .ok_or_else(|| {
                println!("Could not locate voice channel. Guild ID is none");
                PlaybackError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
            })?;
        
        let channel: GuildChannel = ctx.guild_channel()
            .await
            .ok_or_else(|| {
                println!("Could not locate voice channel. Guild channel is none");
                PlaybackError::InternalError("Could not locate voice channel. Guild channel is none".to_owned())
            })?;

        let manager: Arc<Mutex<Call>> = songbird::get(ctx.serenity_context())
            .await
            .ok_or_else(|| {
                println!("Could not locate voice channel. Guild ID is none");
                PlaybackError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
            })?
            .get_or_insert(guild_id);

        if (self.is_playing) {
            self.stop_track(ctx).await?;
        }

        match self.queue.pop() {
            Some(next_track) => {
                println!("- Found: {}", next_track.metadata.title);

                // Send "Now playing message"
                let embed = message_handler::create_now_playing_embed(&next_track);
                let _ = message_handler::send_embed(&ctx, embed, false).await;

                // Play the next track
                let mut guard: MutexGuard<Call> = manager.lock().await;
                let track: YoutubeDl = YoutubeDl::new(ctx.data().request_client.clone(), next_track.metadata.track_url.clone());
                let track_handle: TrackHandle = guard.play(track.into());

                // Add event to handle the track end
                let _ = track_handle
                    .add_event(
                        Event::Track(TrackEvent::End),
                        QueueHandler::new(
                            ctx.serenity_context().clone(),
                            manager.clone(),
                            ctx.data().request_client.clone(),
                            ctx.data().playback.clone(),
                            channel
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
                self.stop_playback(ctx).await?;
                Ok(None)
            }
        }
    }

    fn inner_stop_track(&mut self) -> Result<(), PlaybackError> {
        self.is_playing = false;

        if let Some(track_handle) = &self.track_handle {
            match track_handle.stop() {
                Ok(_) => {
                    println!("- Track stopped");
                }

                Err(e) => {
                    println!("- Error stopping track: {:?}", e);
                    return Err(PlaybackError::InternalError(format!("Error stopping track: {:?}", e)));
                }
            }
        }

        Ok(())
    }

    pub async fn stop_track(&mut self, ctx: Context<'_>) -> Result<(), PlaybackError> {
        println!("Stopping track");

        if !self.is_playing {
            println!("- Playback is not active");
            return Err(PlaybackError::PlaybackNotActive);
        }

        self.inner_stop_track()?;
        self.current_track = None;
        self.track_handle = None;
        
        // Send "Playback stopped message"
        let embed: CreateEmbed = CreateEmbed::new()
            .color(Color::DARK_RED)
            .title("⏹️  Playback stopped")
            .description("Playback has been stopped.")
            .timestamp(Timestamp::now());
        let _ = message_handler::send_embed(&ctx, embed, true).await;

        Ok(())
    }

    pub async fn skip_track(&mut self, ctx: Context<'_>) -> Result<(), PlaybackError> {
        println!("Skipping track");

        if !self.is_playing {
            println!("- Playback is not active");
            return Err(PlaybackError::PlaybackNotActive);
        }

        self.inner_stop_track()?;
        self.current_track = None;
        self.track_handle = None;

        // Hacky way but works :D
        // Event handler does not have a chance to read is_playing as false since its locked
        if (!self.queue.is_empty()) {
            self.is_playing = true;
        }
        
        // Send "Track skipped message"
        let embed: CreateEmbed = CreateEmbed::new()
            .color(Color::DARK_GREEN)
            .title("⏩  Track skipped")
            .description("**Successfully** skipped currently running track!");
        let _ = message_handler::send_embed(&ctx, embed, true).await;

        Ok(())
    }

    pub async fn start_playback(&mut self, ctx: Context<'_>) -> Result<(), PlaybackError> {
        println!("Starting playback");

        if self.is_playing {
            println!("- Playback is already active");
            return Err(PlaybackError::PlaybackAlreadyActive);
        }

        if self.queue.is_empty() {
            println!("- No tracks in queue");
            return Err(PlaybackError::NoTracksInQueue);
        } else {
            println!("- Queue length: {}", self.queue.len());
        }
        
        let _ = self.next_track(ctx).await?;
        Ok(())
    }

    pub async fn stop_playback(&mut self, ctx: Context<'_>) -> Result<(), PlaybackError> {
        println!("Stopping playback");

        if !self.is_playing {
            println!("- Playback is not active");
            return Err(PlaybackError::PlaybackNotActive);
        }

        self.inner_stop_track()?;
        self.current_track = None;
        self.track_handle = None;
        
        // Send "Playback stopped message"
        let embed: CreateEmbed = CreateEmbed::new()
            .color(Color::DARK_RED)
            .title("⏹️  Playback stopped")
            .description("Playback has been stopped.")
            .timestamp(Timestamp::now());
        let _ = message_handler::send_embed(&ctx, embed, true).await;

        Ok(())
    }
    
    pub async fn set_volume(&mut self, ctx: Context<'_>, volume: f32) -> Result<(), PlaybackError> {
        println!("Setting volume to: {}", volume);

        if let Some(track_handle) = &self.track_handle {
            match track_handle.set_volume(volume) {
                Ok(_) => {
                    println!("- Volume set to: {}", volume);
                }

                Err(e) => {
                    println!("- Error setting volume: {:?}", e);
                    return Err(PlaybackError::InternalError(format!("Error setting volume: {:?}", e)));
                }
            }
        }

        Ok(())
    }
}