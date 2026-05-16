use crate::commands;
use crate::commands::{activity, music, reputation, utility};
use crate::handlers::{error_handler, voice_handler};
use crate::player::player::Player;
use crate::player::track::PlaybackError;
use crate::service::gather_service::GatherState;
use crate::service::notifier_service::{Notifier, NotifierError};
use crate::sources::spotify_player::{SpotifyClient, SpotifyError};
use crate::sources::youtube_player::{SearchError, YoutubeClient};
use dotenv::var;
use poise::serenity_prelude;

use serenity::all::{GatewayIntents, GuildId};
use songbird::SerenityInit;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, RwLockWriteGuard};

pub struct MusicBotData {
    pub request_client: reqwest::Client,
    pub youtube_client: YoutubeClient,
    pub spotify_client: SpotifyClient,
    pub database_pool: Arc<Database>,
    pub player: Arc<RwLock<Player>>,
    pub notifier: Arc<RwLock<Notifier>>,
    pub gatherings: Arc<RwLock<HashMap<GuildId, Arc<GatherState>>>>,
}

pub type Database = Pool<Sqlite>;

pub type Context<'a> = poise::Context<'a, MusicBotData, MusicBotError>;

#[derive(Debug, thiserror::Error)]
pub enum MusicBotError {
    #[error("Whoops, an internal error occurred: {0}")]
    InternalError(String),

    #[error("No guild ID found")]
    NoGuildIdError,

    #[error("User not in voice channel")]
    UserNotInVoiceChannelError,

    #[error("Bot not in voice channel")]
    BotNotInVoiceChannelError,

    #[error("Unable to join voice channel")]
    UnableToJoinVoiceChannelError,
}

impl From<serenity_prelude::Error> for MusicBotError {
    fn from(value: serenity_prelude::Error) -> Self {
        MusicBotError::InternalError(value.to_string())
    }
}

impl From<PlaybackError> for MusicBotError {
    fn from(value: PlaybackError) -> Self {
        MusicBotError::InternalError(value.to_string())
    }
}

impl From<MusicBotError> for PlaybackError {
    fn from(value: MusicBotError) -> Self {
        PlaybackError::InternalError(value.to_string())
    }
}

impl From<SearchError> for MusicBotError {
    fn from(value: SearchError) -> Self {
        MusicBotError::InternalError(value.to_string())
    }
}

impl From<SpotifyError> for MusicBotError {
    fn from(value: SpotifyError) -> Self {
        MusicBotError::InternalError(value.to_string())
    }
}

impl From<NotifierError> for MusicBotError {
    fn from(value: NotifierError) -> Self {
        MusicBotError::InternalError(value.to_string())
    }
}

pub struct MusicBotClient {
    serenity_client: serenity_prelude::Client,
}

impl MusicBotClient {
    pub async fn new() -> Self {
        let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILD_MEMBERS | GatewayIntents::GUILD_PRESENCES;

        let discord_token = var("DISCORD_TOKEN").expect("Expected a valid discord token set in the configuration.");

        let database_url = var("DATABASE_URL").expect("Expected a valid database url set in the configuration.");

        let framework = poise::Framework::<MusicBotData, MusicBotError>::builder()
            .options(poise::FrameworkOptions {
                on_error: |err| Box::pin(error_handler::handle(err)),
                commands: vec![
                    commands::help::help(),
                    music::cmd_play::play(),
                    music::cmd_play::play_top(),
                    music::cmd_play::play_now(),
                    music::cmd_pause::pause(),
                    music::cmd_resume::resume(),
                    music::cmd_skip::skip(),
                    music::cmd_stop::stop(),
                    music::cmd_vol::volume(),
                    music::cmd_join::join(),
                    music::cmd_queue::queue(),
                    music::cmd_clear::clear(),
                    music::cmd_remove::remove(),
                    music::cmd_leave::leave(),
                    music::cmd_shuffle::shuffle(),
                    music::cmd_playing::playing(),
                    music::cmd_history::history(),
                    music::cmd_local::local(),
                    music::cmd_silent::silent(),
                    music::cmd_normalize::normalize(),
                    utility::cmd_uwu::uwu(),
                    utility::cmd_uwu::uwu_me(),
                    activity::cmd_gather::gather(),
                    utility::cmd_notify::notify(),
                    utility::cmd_notify::remind(),
                    utility::cmd_wakeup::wakeup(),
                    utility::cmd_wakeup::wakeup_context(),
                    utility::cmd_rename::rename(),
                    reputation::cmd_plus::add_rep(),
                    reputation::cmd_minus::remove_rep(),
                    reputation::cmd_list::list_rep(),
                    utility::cmd_rename::rename_context(),
                ],
                pre_command: |ctx| {
                    Box::pin(async move {
                        tracing::info!(
                            "CMD: {} is executing {} ({})",
                            ctx.author().name,
                            ctx.command().name,
                            ctx.invocation_string()
                        );
                    })
                },
                post_command: |ctx| {
                    Box::pin(async move {
                        error_handler::schedule_prefix_delete(ctx);
                    })
                },
                event_handler: |ctx, event, _fw, data| Box::pin(async move { voice_handler::handle(ctx, event, data).await }),

                prefix_options: poise::PrefixFrameworkOptions {
                    prefix: Some(String::from("!")),
                    ..Default::default()
                },
                ..Default::default()
            })
            .setup(move |ctx, ready, fw| {
                Box::pin(async move {
                    let guild_id: GuildId = ready.guilds[0].id;
                    let guild_id_map: i64 = guild_id.get() as i64;

                    tracing::info!("Bot ready");
                    tracing::info!("Logged in as {}", ready.user.name);

                    crate::player::player::set_idle(ctx);

                    tracing::info!("Registering commands in guild");
                    poise::builtins::register_in_guild(ctx, &fw.options().commands, ready.guilds[0].id)
                        .await
                        .map_err(|e| {
                            tracing::error!("Failed to register commands in guild: {:?}", e);
                            MusicBotError::InternalError(e.to_string())
                        })?;

                    tracing::info!("Connecting to database");
                    let database: Arc<Database> = Arc::new(
                        SqlitePoolOptions::new()
                            .connect(&database_url)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to connect to database: {:?}", e);
                                MusicBotError::InternalError(e.to_string())
                            })?,
                    );

                    tracing::info!("Running database migrations");
                    sqlx::migrate!("./migrations")
                        .run(&*database)
                        .await
                        .map_err(|e| {
                            tracing::error!("Failed to run migrations: {:?}", e);
                            MusicBotError::InternalError(e.to_string())
                        })?;

                    let _ = sqlx::query!(
                        "INSERT OR IGNORE INTO guilds (guild_id, volume) VALUES ($1, $2)",
                        guild_id_map,
                        0.5
                    )
                    .execute(&*database)
                    .await
                    .map_err(|e| {
                        tracing::error!("Failed to insert guild into database: {:?}", e);
                        MusicBotError::InternalError(e.to_string())
                    })?;

                    let player: Player = Player::new(guild_id, database.clone()).await;
                    let player_handle: Arc<RwLock<Player>> = Arc::new(RwLock::new(player));

                    let notifier: Notifier = Notifier::new(ctx.clone(), database.clone()).await;
                    let notifier_handle: Arc<RwLock<Notifier>> = Arc::new(RwLock::new(notifier));
                    let notifier_handle_clone: Arc<RwLock<Notifier>> = Arc::clone(&notifier_handle);

                    tokio::spawn(async move {
                        loop {
                            let mut notifier: RwLockWriteGuard<Notifier> = notifier_handle_clone.write().await;
                            notifier.check_messages().await;
                            drop(notifier);

                            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                        }
                    });

                    Ok(MusicBotData {
                        request_client: reqwest::Client::new(),
                        youtube_client: YoutubeClient::new(),
                        spotify_client: SpotifyClient::new(),
                        database_pool: database,
                        player: player_handle,
                        notifier: notifier_handle,
                        gatherings: Arc::new(RwLock::new(HashMap::new())),
                    })
                })
            })
            .build();

        let serenity_client = serenity_prelude::Client::builder(discord_token, intents)
            .register_songbird()
            .framework(framework)
            .await
            .expect("Failed to build serenity client.");

        Self { serenity_client }
    }

    pub async fn start(&mut self) -> Result<(), MusicBotError> {
        tracing::info!("Starting bot client");

        let shard_manager = self.serenity_client.shard_manager.clone();
        tokio::spawn(async move {
            wait_for_signal().await;
            tracing::info!("Shutdown signal received, disconnecting bot...");
            shard_manager.shutdown_all().await;
        });

        self.serenity_client.start().await.map_err(|e| {
            tracing::error!("Failed to start server: {:?}", e);
            MusicBotError::InternalError(e.to_string())
        })
    }
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to listen for SIGTERM");
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to listen for SIGINT");
    tokio::select! {
        _ = sigterm.recv() => tracing::info!("Received SIGTERM"),
        _ = sigint.recv()  => tracing::info!("Received SIGINT"),
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    tracing::info!("Received Ctrl+C");
}
