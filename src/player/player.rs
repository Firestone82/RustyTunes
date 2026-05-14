use crate::bot::{Context, Database, MusicBotError};
use crate::embeds::player_embed::PlayerEmbed;
use crate::handlers::queue_handler::QueueHandler;
use crate::service::cache_service;
use crate::service::embed_service::SendEmbed;
use poise::serenity_prelude as serenity_prelude;
use rand::seq::SliceRandom;
use serenity::all::{ActivityData, GuildId};
use songbird::input::{File, Input, YoutubeDl};
use songbird::tracks::TrackHandle;
use songbird::{Call, Event, TrackEvent};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};

#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    // Bare display string — `MusicBotError::InternalError` already adds the
    // user-facing "Whoops…" prefix when this is converted at the boundary.
    #[error("{0}")]
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

    #[error("Invalid queue index: {0}")]
    InvalidQueueIndex(usize),
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
    pub metadata: TrackMetadata,
    pub added_by: String,
    pub source: TrackSource,
}

#[derive(Debug, Clone)]
pub enum TrackSource {
    /// Streamed via yt-dlp from a YouTube URL.
    YouTube,
    /// Resolved from Spotify, played via yt-dlp's `ytsearch1:` prefix.
    Spotify,
    /// A previously downloaded file on the local filesystem.
    Local(PathBuf),
}

impl TrackSource {
    pub fn label(&self) -> &'static str {
        match self {
            TrackSource::YouTube => "YouTube",
            TrackSource::Spotify => "Spotify",
            TrackSource::Local(_) => "Local file",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            TrackSource::YouTube => "🎬",
            TrackSource::Spotify => "🟢",
            TrackSource::Local(_) => "📁",
        }
    }
}

impl Track {
    /// Pick the best input for this track:
    ///   1. If a raw cache exists, play that.
    ///   2. Else stream through yt-dlp and kick off a background cache write
    ///      so the next play of the same track is a cheap file read.
    pub async fn resolve_input(&self, req_client: &reqwest::Client) -> Input {
        if let TrackSource::Local(path) = &self.source {
            return File::new(path.clone()).into();
        }

        if let Some(raw) = cache_service::find_cached(self).await {
            return File::new(raw).into();
        }

        // Cache miss: stream now, fill the cache for next time.
        // `play_url` lets sources (like Spotify) ship a yt-dlp-friendly input
        // string while keeping `track_url` set to the user-facing permalink.
        let input_url = self
            .metadata
            .play_url
            .clone()
            .unwrap_or_else(|| self.metadata.track_url.clone());
        cache_service::spawn_cache(self.clone());
        YoutubeDl::new(req_client.clone(), input_url).into()
    }
}

#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub track_url: String,
    /// Optional override used by `build_input`. For Spotify this is the
    /// `ytsearch1:` query, while `track_url` stays the Spotify permalink.
    pub play_url: Option<String>,
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
    /// Session-only "shh" mode — when on, the NowPlaying embed is suppressed.
    /// Resets to `false` on bot restart.
    pub silent: bool,
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
            silent: false,
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

    pub async fn add_playlist_to_queue(&mut self, ctx: Context<'_>, playlist: Playlist, top: bool) -> Result<(), PlaybackError> {
        tracing::info!("Adding playlist to queue (top={}), tracks: {}", top, playlist.tracks.len());

        self.inactivity_cancel.store(true, Ordering::SeqCst);
        if top {
            self.queue.splice(0..0, playlist.tracks);
        } else {
            self.queue.extend(playlist.tracks);
        }
        tracing::debug!("Queue length: {}", self.queue.len());

        self.kick_off_playback(ctx, top).await
    }

    pub async fn add_track_to_queue(&mut self, ctx: Context<'_>, track: Track, top: bool) -> Result<(), PlaybackError> {
        tracing::info!("Adding track to queue (top={}): {}", top, track.metadata.track_url);

        self.inactivity_cancel.store(true, Ordering::SeqCst);
        if top {
            self.queue.insert(0, track);
        } else {
            self.queue.push(track);
        }
        tracing::debug!("Queue length: {}", self.queue.len());

        self.kick_off_playback(ctx, top).await
    }

    /// Decide what to do after appending to the queue:
    /// - paused + top:    skip the currently-paused track and play the new one immediately
    /// - paused + !top:   resume the currently-paused track
    /// - idle:            start playback from the head of the queue
    /// - already playing: nothing to do
    async fn kick_off_playback(&mut self, ctx: Context<'_>, top: bool) -> Result<(), PlaybackError> {
        if self.is_paused {
            if top {
                self.is_paused = false;
                self.next_track(ctx).await?;
            } else {
                self.resume().await?;
            }
        } else if !self.is_playing {
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

                // Send "Now playing message" unless the guild has session-only silent mode on.
                if !self.silent {
                    PlayerEmbed::NowPlaying(&next_track)
                        .to_embed()
                        .send_context(ctx, false, Some(30)).await?;
                }

                let input = next_track
                    .resolve_input(&ctx.data().request_client)
                    .await;

                // Play the next track
                let mut guard: MutexGuard<Call> = manager
                    .lock()
                    .await;

                let track_handle: TrackHandle = guard.play(input.into());
                
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

                set_now_playing(ctx.serenity_context(), &next_track);

                self.push_to_history(next_track.clone());
                self.current_track = Some(next_track);
                self.track_handle = Some(track_handle);
                self.is_playing = true;

                Ok(self.current_track.as_ref())
            }

            None => {
                tracing::info!("No more tracks to play. Stopping playback");
                set_idle(ctx.serenity_context());
                self.stop_playback().await?;
                Ok(None)
            }
        }
    }

    pub async fn clear_queue(&mut self) -> usize {
        let cleared = self.queue.len();
        tracing::info!("Clearing queue ({} tracks)", cleared);
        self.queue.clear();
        cleared
    }

    pub async fn remove_from_queue(&mut self, index: usize) -> Result<Track, PlaybackError> {
        tracing::info!("Removing track at queue index {}", index);

        if index == 0 || index > self.queue.len() {
            return Err(PlaybackError::InvalidQueueIndex(index));
        }

        Ok(self.queue.remove(index - 1))
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

/// Set the bot's Discord activity. We bake the "Playing " word into the label
/// itself because some Discord clients hide the activity-type prefix on bots.
pub fn set_now_playing(ctx: &serenity_prelude::Context, track: &Track) {
    let label = format!("Playing {} · {}", track.metadata.title, track.source.label());
    ctx.set_activity(Some(ActivityData::playing(label)));
}

/// Friendly default status shown whenever the bot isn't playing anything.
pub fn set_idle(ctx: &serenity_prelude::Context) {
    ctx.set_activity(Some(ActivityData::listening("!help · waiting for !play")));
}