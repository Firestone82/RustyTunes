use crate::bot::{Context, Database, MusicBotError};
use crate::embeds::player_embed::PlayerEmbed;
use crate::handlers::queue_handler::QueueHandler;
use crate::service::embed_service::SendEmbed;
use rand::seq::SliceRandom;
use serenity::all::GuildId;
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
    pub track_handle: Option<TrackHandle>,
    pub current_track: Option<Track>,
    pub queue: Vec<Track>,
    pub volume: f32,
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
                println!("Failed to fetch volume from database. Error: {:?}", e);
                MusicBotError::InternalError(e.to_string())
            });

        let volume: f32 = match volume {
            Ok(volume) => volume.volume.unwrap_or(0.5) as f32,
            Err(_) => 0.5
        };

        Player {
            is_playing: false,
            track_handle: None,
            current_track: None,
            queue: Vec::new(),
            volume,
            guild_id,
            database
        }
    }

    pub async fn add_playlist_to_queue(&mut self, ctx: Context<'_>, playlist: Playlist) -> Result<(), PlaybackError> {
        println!("Adding playlist to queue, tracks: {}", playlist.tracks.len());

        self.queue.extend(playlist.tracks);
        println!("- Queue length: {}", self.queue.len());
        
        if !self.is_playing {
            self.start_playback(ctx).await?;
        }
        
        Ok(())
    }
    
    pub async fn add_track_to_queue(&mut self, ctx: Context<'_>, track: Track) -> Result<(), PlaybackError> {
        println!("Adding track to queue: {}", track.metadata.track_url);

        self.queue.push(track);
        println!("- Queue length: {}", self.queue.len());

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

        if self.queue.is_empty() || self.is_playing {
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

        self.is_playing = true;
        self.next_track(ctx).await?;
        
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
                        ctx.guild_channel().await.unwrap()
                    )
                );

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

    pub async fn shuffle(&mut self) -> Result<(), PlaybackError> {
        println!("Shuffling queue");

        if self.queue.len() > 1 {
            let mut rng = rand::thread_rng();
            self.queue.shuffle(&mut rng);
        }
        
        Ok(())
    }
    
    pub async fn set_volume(&mut self, mut volume: f32) -> Result<(), PlaybackError> {
        println!("Setting volume to: {:?}", volume);

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

    pub async fn stop_track(&mut self) -> Result<(), PlaybackError> {
        if self.is_playing {
            if let Some(track_handle) = &self.track_handle {
                println!("Stopping track");
                
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