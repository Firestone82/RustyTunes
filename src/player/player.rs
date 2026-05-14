use crate::bot::{Context, Database, MusicBotError};
use crate::embeds::player_embed::PlayerEmbed;
use crate::handlers::queue_handler::QueueHandler;
use crate::service::cache_service;
use crate::service::embed_service::SendEmbed;
use crate::service::normalize_service;
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
    ///   2. Else stream through yt-dlp; the caller is expected to kick off
    ///      a background cache-and-normalize pass via
    ///      `spawn_cache_and_apply` so the gain can be applied mid-track
    ///      as soon as the cache is ready.
    ///
    /// Returns the chosen `Input` along with the on-disk path it was built
    /// from (when available). The path is what loudness normalization needs;
    /// streamed inputs return `None`.
    pub async fn resolve_input(&self, req_client: &reqwest::Client) -> (Input, Option<PathBuf>) {
        if let TrackSource::Local(path) = &self.source {
            return (File::new(path.clone()).into(), Some(path.clone()));
        }

        if let Some(raw) = cache_service::find_cached(self).await {
            let path = raw.clone();
            return (File::new(raw).into(), Some(path));
        }

        // Cache miss: stream now. The caller fires off the cache write.
        // `play_url` lets sources (like Spotify) ship a yt-dlp-friendly input
        // string while keeping `track_url` set to the user-facing permalink.
        let input_url = self
            .metadata
            .play_url
            .clone()
            .unwrap_or_else(|| self.metadata.track_url.clone());
        (YoutubeDl::new(req_client.clone(), input_url).into(), None)
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
    /// Multiplier applied on top of `volume` for the current track to even
    /// out perceived loudness across songs. `1.0` when normalization is off
    /// or no measurement is available yet. Updated asynchronously when a
    /// fresh measurement comes back from ffmpeg.
    pub current_gain: f32,
    /// On-disk path of the currently playing track (cached file or local
    /// file). Used by `!normalize` to re-measure and apply gain mid-track.
    /// `None` for streamed inputs that have no analyzable file yet.
    pub current_source_path: Option<PathBuf>,
    pub inactivity_cancel: Arc<AtomicBool>,
    /// Session-only "shh" mode — when on, the NowPlaying embed is suppressed.
    /// Resets to `false` on bot restart.
    pub silent: bool,
    /// Session-only loudness normalization toggle. Off by default and reset
    /// to `false` on bot restart — flip it on with `!normalize` for the
    /// session. When on, it applies to every source (YouTube, Spotify, and
    /// local files) for every track that has a measurable file path.
    pub normalize: bool,
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
            current_gain: 1.0,
            current_source_path: None,
            inactivity_cancel: Arc::new(AtomicBool::new(false)),
            silent: false,
            normalize: false,
            guild_id,
            database
        }
    }

    /// Whether loudness normalization should apply this session.
    pub fn should_normalize(&self) -> bool {
        self.normalize
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

                let (input, source_path) = next_track
                    .resolve_input(&ctx.data().request_client)
                    .await;

                // Play the next track
                let mut guard: MutexGuard<Call> = manager
                    .lock()
                    .await;

                let track_handle: TrackHandle = guard.play(input.into());

                // Reset per-track gain. There are two cases:
                //   - File on disk (cache hit or local): measure & apply (if
                //     normalize is on) using the existing path.
                //   - Streaming (cache miss): spawn the cache download; the
                //     helper records the path on the player and applies the
                //     gain to the live track handle as soon as ffmpeg
                //     finishes, so first plays get normalized too.
                self.current_gain = 1.0;
                self.current_source_path = source_path.clone();
                let _ = track_handle.set_volume(self.volume);

                match source_path {
                    Some(path) => {
                        if self.should_normalize() {
                            schedule_normalization_apply(
                                ctx.data().player.clone(),
                                track_handle.clone(),
                                path,
                                next_track.id.clone(),
                            );
                        }
                    }
                    None => {
                        spawn_cache_and_apply(
                            next_track.clone(),
                            ctx.data().player.clone(),
                            track_handle.clone(),
                        );
                    }
                }

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
            let _ = track_handle.set_volume(volume * self.current_gain);
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
        self.current_source_path = None;
        self.current_gain = 1.0;

        Ok(())
    }

    pub async fn stop_playback(&mut self) -> Result<(), PlaybackError> {
        self.stop_track().await?;
        self.queue.clear();

        Ok(())
    }
}

/// Spawn a task that measures `path`'s loudness, then applies the resulting
/// multiplier to `handle` provided the player is still on `track_id` and the
/// normalize toggle is still on by the time the measurement returns. Used
/// both when a track starts and when `!normalize` is flipped mid-track.
pub fn schedule_normalization_apply(
    player_arc: Arc<tokio::sync::RwLock<Player>>,
    handle: TrackHandle,
    path: PathBuf,
    track_id: String,
) {
    tokio::spawn(async move {
        let measurement = normalize_service::measurement_for(&path).await;
        let mut player = player_arc.write().await;
        let still_current = player
            .current_track
            .as_ref()
            .map(|t| t.id == track_id)
            .unwrap_or(false);
        if !still_current || !player.should_normalize() {
            return;
        }
        let title = player
            .current_track
            .as_ref()
            .map(|t| t.metadata.title.clone())
            .unwrap_or_default();
        player.current_gain = measurement.multiplier;
        let effective = player.volume * measurement.multiplier;
        let _ = handle.set_volume(effective);
        let lufs_str = measurement
            .lufs
            .map(|l| format!("{l:.2} LUFS"))
            .unwrap_or_else(|| "unknown LUFS".to_string());
        tracing::info!(
            "Normalize applied: '{}' — {} → gain {:+.2} dB (×{:.3}); volume {:.0}% × gain = {:.3} effective",
            title,
            lufs_str,
            measurement.gain_db,
            measurement.multiplier,
            player.volume * 100.0,
            effective,
        );
    });
}

/// Streaming first-play helper: caches `track` in the background and, once
/// the file lands, records its path on the player and schedules a loudness
/// measurement so normalization can apply to the currently playing track
/// without having to wait for the next play. A no-op for tracks that aren't
/// cacheable (local files, or anything missing an id).
pub fn spawn_cache_and_apply(
    track: Track,
    player_arc: Arc<tokio::sync::RwLock<Player>>,
    handle: TrackHandle,
) {
    if !cache_service::is_cacheable(&track) {
        return;
    }
    tokio::spawn(async move {
        match cache_service::cache_track(&track).await {
            Ok(path) => {
                tracing::info!(
                    "Cached '{}' to {}",
                    track.metadata.title,
                    path.display()
                );

                // Stash the path on the player so future `!normalize` toggles
                // can find the file — but only if the user hasn't already
                // skipped to a different track.
                {
                    let mut player = player_arc.write().await;
                    let still_current = player
                        .current_track
                        .as_ref()
                        .map(|t| t.id == track.id)
                        .unwrap_or(false);
                    if still_current {
                        player.current_source_path = Some(path.clone());
                    }
                }

                // Measure (result is cached for next play) and apply if the
                // user has normalization on and the track is still current.
                schedule_normalization_apply(player_arc, handle, path, track.id);
            }
            Err(e) => tracing::warn!(
                "Failed to cache '{}': {}",
                track.metadata.title,
                e
            ),
        }
    });
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