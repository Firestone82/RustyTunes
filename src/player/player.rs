use crate::bot::{Context, Database, MusicBotError};
use crate::embeds::player_embed::PlayerEmbed;
use crate::handlers::queue_handler::QueueHandler;
use crate::service::embed_service::SendEmbed;
use rand::seq::SliceRandom;
use serenity::all::GuildId;
use songbird::input::YoutubeDl;
use songbird::tracks::TrackHandle;
use songbird::{Call, Event, TrackEvent};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
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
    PlaybackAlreadyActive,

    #[error("Playback is already paused")]
    PlaybackAlreadyPaused,

    #[error("Playback is not paused")]
    PlaybackNotPaused,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub description: String,
    pub playlist_url: String,
    pub tracks: Vec<Track>
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: String,
    pub metadata: TrackMetadata
}

#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub track_url: String,
}

pub struct Player {
    pub is_playing: bool,
    pub is_paused: bool,
    pub track_handle: Option<TrackHandle>,
    pub current_track: Option<Track>,
    pub queue: Vec<Track>,
    pub history: VecDeque<Track>,
    pub volume: f32,
    pub inactivity_cancel: Arc<AtomicBool>,
    guild_id: GuildId,
    database: Arc<Database>,
}

impl Player {
    pub async fn new(guild_id: GuildId, database: Arc<Database>) -> Self {
        let guild_id_map: i64 = guild_id.get() as i64;

        let volume = sqlx::query!(
            "SELECT * FROM guilds WHERE guild_id = $1",
            guild_id_map
        ).fetch_one(&*database)
            .await
            .map_err(|e| {
                tracing::error!("Failed to fetch volume from database: {:?}", e);
                MusicBotError::InternalError(e.to_string())
            });

        let volume: f32 = match volume {
            Ok(volume) => volume.volume.unwrap_or(0.5) as f32,
            Err(_) => 0.5
        };

        Player {
            is_playing: false,
            is_paused: false,
            track_handle: None,
            current_track: None,
            queue: Vec::new(),
            history: VecDeque::new(),
            volume,
            inactivity_cancel: Arc::new(AtomicBool::new(false)),
            guild_id,
            database
        }
    }

    pub fn push_to_history(&mut self, track: Track) {
        self.history.push_back(track);
        if self.history.len() > 10 {
            self.history.pop_front();
        }
    }

    pub async fn add_playlist_to_queue(&mut self, ctx: Context<'_>, playlist: Playlist) -> Result<(), PlaybackError> {
        tracing::info!("Adding playlist to queue, tracks: {}", playlist.tracks.len());

        self.inactivity_cancel.store(true, Ordering::SeqCst);
        self.queue.extend(playlist.tracks);
        tracing::debug!("Queue length: {}", self.queue.len());

        if !self.is_playing {
            self.start_playback(ctx).await?;
        }

        Ok(())
    }

    pub async fn add_track_to_queue(&mut self, ctx: Context<'_>, track: Track) -> Result<(), PlaybackError> {
        tracing::info!("Adding track to queue: {}", track.metadata.track_url);

        self.inactivity_cancel.store(true, Ordering::SeqCst);
        self.queue.push(track);
        tracing::debug!("Queue length: {}", self.queue.len());

        if !self.is_playing {
            self.start_playback(ctx).await?;
        }

        Ok(())
    }

    pub async fn skip(&mut self, mut amount: usize) -> Result<usize, PlaybackError> {
        tracing::info!("Skipping {} track(s)", amount);

        if !self.is_playing {
            tracing::debug!("Playback is not active");
            return Err(PlaybackError::PlaybackNotActive);
        }

        if amount > self.queue.len() {
            tracing::debug!("Amount to skip is greater than queue length. Skipping all tracks");
            amount = amount.min(self.queue.len());
        }

        if self.queue.is_empty() && self.is_playing {
            tracing::info!("No tracks in queue. Stopping playback");
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

        self.is_playing = true;
        self.next_track(ctx).await?;
        
        Ok(())
    }

    pub async fn next_track(&mut self, ctx: Context<'_>) -> Result<Option<&Track>, PlaybackError> {
        tracing::info!("Requesting next track to play");

        let guild_id: GuildId = ctx.guild_id()
            .ok_or_else(|| {
                tracing::error!("Could not locate voice channel: guild ID is none");
                PlaybackError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
            })?;

        let manager: Arc<Mutex<Call>> = songbird::get(ctx.serenity_context()).await
            .ok_or_else(|| {
                tracing::error!("Could not locate voice channel: guild ID is none");
                PlaybackError::InternalError("Could not locate voice channel. Guild ID is none".to_owned())
            })?
            .get_or_insert(guild_id);

        if self.is_playing {
            self.stop_track().await?;
        }

        let next = if self.queue.is_empty() { None } else { Some(self.queue.remove(0)) };
        match next {
            Some(next_track) => {
                tracing::info!("Found: {}", next_track.metadata.title);

                // Send "Now playing message"
                PlayerEmbed::NowPlaying(&next_track)
                    .to_embed()
                    .send_context(ctx, false, Some(30)).await?;

                // Play the next track
                let mut guard: MutexGuard<Call> = manager
                    .lock()
                    .await;

                let track_data: YoutubeDl = YoutubeDl::new(ctx.data().request_client.clone(), next_track.metadata.track_url.clone());
                let track_handle: TrackHandle = guard.play(track_data.into());
                
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
                        ctx.guild_channel().await.unwrap(),
                        guild_id,
                    )
                );

                self.push_to_history(next_track.clone());
                self.current_track = Some(next_track);
                self.track_handle = Some(track_handle);
                self.is_playing = true;

                Ok(self.current_track.as_ref())
            }

            None => {
                tracing::info!("No more tracks to play. Stopping playback");
                self.stop_playback().await?;
                Ok(None)
            }
        }
    }

    pub async fn shuffle(&mut self) -> Result<(), PlaybackError> {
        tracing::info!("Shuffling queue");

        if self.queue.len() > 1 {
            let mut rng = rand::rng();
            self.queue.shuffle(&mut rng);
        }
        
        Ok(())
    }
    
    pub async fn set_volume(&mut self, mut volume: f32) -> Result<(), PlaybackError> {
        tracing::info!("Setting volume to: {:?}", volume);

        // Normalize volume
        volume /= 100.0;
        volume = volume.max(0.0);
        
        if let Some(track_handle) = &self.track_handle {
            let _ = track_handle.set_volume(volume);
        }

        let guild_id_map: i64 = self.guild_id.get() as i64;

        sqlx::query!(
            "UPDATE guilds SET volume = $1 WHERE guild_id = $2",
            volume, guild_id_map
        ).execute(&*self.database).await.expect("TODO: panic message");

        self.volume = volume;
        Ok(())
    }

    pub async fn pause(&mut self) -> Result<(), PlaybackError> {
        if !self.is_playing {
            return Err(PlaybackError::PlaybackNotActive);
        }
        if self.is_paused {
            return Err(PlaybackError::PlaybackAlreadyPaused);
        }
        if let Some(track_handle) = &self.track_handle {
            track_handle.pause().map_err(|e| PlaybackError::InternalError(e.to_string()))?;
        }
        self.is_paused = true;
        Ok(())
    }

    pub async fn resume(&mut self) -> Result<(), PlaybackError> {
        if !self.is_paused {
            return Err(PlaybackError::PlaybackNotPaused);
        }
        if let Some(track_handle) = &self.track_handle {
            track_handle.play().map_err(|e| PlaybackError::InternalError(e.to_string()))?;
        }
        self.is_paused = false;
        Ok(())
    }

    pub async fn stop_track(&mut self) -> Result<(), PlaybackError> {
        if self.is_playing {
            if let Some(track_handle) = &self.track_handle {
                tracing::info!("Stopping track");

                if let Err(error) = track_handle.stop() {
                    tracing::error!("Error stopping track: {:?}", error);
                    return Err(PlaybackError::InternalError(format!("Error stopping track: {:?}", error)));
                }
            }
        }

        self.is_playing = false;
        self.is_paused = false;
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