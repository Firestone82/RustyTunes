use crate::commands;
use crate::commands::{music, reputation, utility};
use crate::embeds::bot_embeds::BotEmbed;
use crate::handlers::error_handler;
use crate::player::notifier::{Notifier, NotifierError};
use crate::player::player::{PlaybackError, Player};
use crate::service::embed_service::SendEmbed;
use crate::sources::spotify::spotify_client::{SpotifyClient, SpotifyError};
use crate::sources::youtube::youtube_client::{SearchError, YoutubeClient};
use dotenv::var;
use poise::serenity_prelude;

use serenity::all::{
    ChannelId, FullEvent, GatewayIntents, GuildId, MemberAction, Mentionable,
};
use songbird::SerenityInit;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tokio::sync::{RwLock, RwLockWriteGuard};

pub struct MusicBotData {
    pub request_client: reqwest::Client,
    pub youtube_client: YoutubeClient,
    pub spotify_client: SpotifyClient,
    pub database_pool: Arc<Database>,
    pub player: Arc<RwLock<Player>>,
    pub notifier: Arc<RwLock<Notifier>>,
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
        let intents = GatewayIntents::non_privileged()
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_VOICE_STATES
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_PRESENCES;

        let discord_token =
            var("DISCORD_TOKEN").expect("Expected a valid discord token set in the configuration.");

        let database_url =
            var("DATABASE_URL").expect("Expected a valid database url set in the configuration.");

        let framework = poise::Framework::<MusicBotData, MusicBotError>::builder()
            .options(poise::FrameworkOptions {
                on_error: |err| Box::pin(error_handler::handle(err)),
                commands: vec![
                    commands::help::help(),
                    music::cmd_play::play(),
                    music::cmd_play::play_top(),
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
                pre_command: |ctx| Box::pin(async move {
                    tracing::info!("CMD: {} is executing {} ({})", ctx.author().name, ctx.command().name, ctx.invocation_string());
                }),
                post_command: |ctx| Box::pin(async move {
                    error_handler::schedule_prefix_delete(ctx);
                }),
                event_handler: |ctx, event, _fw, data| Box::pin(async move {
                    if let FullEvent::VoiceStateUpdate { new, .. } = event {
                        let guild_id = match new.guild_id {
                            Some(g) => g,
                            None => return Ok(()),
                        };

                        let bot_id = ctx.cache.current_user().id;

                        let bot_channel: Option<ChannelId> = ctx.cache
                            .guild(guild_id)
                            .as_ref()
                            .and_then(|g| g.voice_states.get(&bot_id))
                            .and_then(|vs| vs.channel_id);

                        // Bot lost its voice channel (kicked, dragged out, server-mute disconnect).
                        // Treat it the same as a normal disconnect: stop playback (also covers a
                        // paused track, which still holds queue state), wipe the queue, and drop
                        // the songbird call.
                        if bot_channel.is_none() {
                            let mut player = data.player.write().await;
                            let needs_cleanup = player.is_playing
                                || player.is_paused
                                || !player.queue.is_empty();

                            if needs_cleanup {
                                tracing::info!("Bot is no longer in a voice channel. Cleaning up playback state.");
                                let _ = player.stop_playback().await;
                                drop(player);
                                crate::player::player::set_idle(ctx);

                                if let Some(manager) = songbird::get(ctx).await {
                                    let _ = manager.remove(guild_id).await;
                                }
                            }
                            return Ok(());
                        }

                        let bot_channel = bot_channel.unwrap();

                        let humans = ctx.cache
                            .guild(guild_id)
                            .as_ref()
                            .map(|g| g.voice_states.values()
                                .filter(|vs| vs.channel_id == Some(bot_channel) && vs.user_id != bot_id)
                                .count())
                            .unwrap_or(0);

                        if humans == 0 {
                            tracing::info!("Bot is alone in voice channel. Leaving.");

                            let _ = data.player.write().await.stop_playback().await;
                            crate::player::player::set_idle(ctx);

                            if let Some(manager) = songbird::get(ctx).await {
                                let _ = manager.remove(guild_id).await;
                            }
                        }
                    }

                    Ok(())
                }),

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
                            })?
                    );

                    // Schema patch: relax notify_me.message_id NOT NULL so slash commands
                    // (which have no source message) can store NULL.
                    //
                    // The whole patch runs in one transaction on one connection. Done that
                    // way for two reasons: (1) SQLite's per-connection schema cache can lag
                    // on Windows when statements hop between pool connections, which made
                    // an earlier autocommit version blow up at the final ALTER TABLE
                    // RENAME; (2) it lets us recover cleanly if a prior run failed half-way
                    // and left `notify_me_new` behind without `notify_me`.
                    let new_table_present = table_exists(&database, "notify_me_new").await?;
                    let old_table_present = table_exists(&database, "notify_me").await?;

                    let message_id_notnull: Option<i64> = if old_table_present {
                        sqlx::query_scalar(
                            "SELECT \"notnull\" FROM pragma_table_info('notify_me') WHERE name = 'message_id'"
                        ).fetch_optional(&*database)
                            .await
                            .map_err(|e| MusicBotError::InternalError(format!("Schema probe failed: {e}")))?
                    } else {
                        None
                    };

                    let needs_full_migration = matches!(message_id_notnull, Some(1));

                    if new_table_present || needs_full_migration {
                        tracing::info!(
                            "Migrating notify_me.message_id to NULL-able (recovery={})",
                            new_table_present && !needs_full_migration
                        );

                        let mut tx = database.begin().await
                            .map_err(|e| MusicBotError::InternalError(format!("Begin migration tx failed: {e}")))?;

                        // Recovery path: a previous run already produced notify_me_new.
                        // Just drop whatever notify_me exists and rename.
                        if new_table_present {
                            if old_table_present {
                                sqlx::query("DROP TABLE notify_me").execute(&mut *tx).await
                                    .map_err(|e| MusicBotError::InternalError(format!("DROP notify_me failed: {e}")))?;
                            }
                            sqlx::query("ALTER TABLE notify_me_new RENAME TO notify_me")
                                .execute(&mut *tx).await
                                .map_err(|e| MusicBotError::InternalError(format!("RENAME notify_me_new failed: {e}")))?;
                        } else {
                            // Full migration.
                            sqlx::query(
                                "CREATE TABLE notify_me_new (\
                                    id INTEGER PRIMARY KEY AUTOINCREMENT,\
                                    guild_id INTEGER NOT NULL,\
                                    channel_id INTEGER NOT NULL,\
                                    user_id INTEGER NOT NULL,\
                                    message_id INTEGER,\
                                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,\
                                    notify_at DATETIME,\
                                    note TEXT DEFAULT NULL\
                                )"
                            ).execute(&mut *tx).await
                                .map_err(|e| MusicBotError::InternalError(format!("CREATE notify_me_new failed: {e}")))?;

                            sqlx::query(
                                "INSERT INTO notify_me_new (id, guild_id, channel_id, user_id, message_id, created_at, notify_at, note) \
                                    SELECT id, guild_id, channel_id, user_id, message_id, created_at, notify_at, note FROM notify_me"
                            ).execute(&mut *tx).await
                                .map_err(|e| MusicBotError::InternalError(format!("Copy into notify_me_new failed: {e}")))?;

                            sqlx::query("DROP TABLE notify_me").execute(&mut *tx).await
                                .map_err(|e| MusicBotError::InternalError(format!("DROP notify_me failed: {e}")))?;

                            sqlx::query("ALTER TABLE notify_me_new RENAME TO notify_me")
                                .execute(&mut *tx).await
                                .map_err(|e| MusicBotError::InternalError(format!("RENAME notify_me_new failed: {e}")))?;
                        }

                        tx.commit().await
                            .map_err(|e| MusicBotError::InternalError(format!("Commit migration tx failed: {e}")))?;
                    }

                    // Insert guild into database if it doesn't exist
                    let _ = sqlx::query!(
                        "INSERT OR IGNORE INTO guilds (guild_id, volume) VALUES ($1, $2)",
                        guild_id_map, 0.5
                    ).execute(&*database)
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

                    // Start notifier scheduler
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

        self.serenity_client.start().await.map_err(|e| {
            tracing::error!("Failed to start server: {:?}", e);
            MusicBotError::InternalError(e.to_string())
        })?;
        let shard_manager = self.serenity_client.shard_manager.clone();
        tokio::spawn(async move {
            wait_for_signal().await;
            tracing::info!("Shutdown signal received, disconnecting bot...");
            shard_manager.shutdown_all().await;
        });

        self.serenity_client.start().await
            .map_err(|e| {
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
    tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    tracing::info!("Received Ctrl+C");
}

async fn table_exists(database: &Arc<Database>, name: &str) -> Result<bool, MusicBotError> {
    let row: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(name)
            .fetch_optional(&**database)
            .await
            .map_err(|e| MusicBotError::InternalError(format!("Catalog probe failed: {e}")))?;
    Ok(row.is_some())
}
