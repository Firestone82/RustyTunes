use crate::commands;
use crate::commands::{music, utility};
use crate::embeds::bot_embeds::BotEmbed;
use crate::handlers::error_handler;
use crate::player::notifier::{Notifier, NotifierError};
use crate::player::player::{PlaybackError, Player};
use crate::service::embed_service::SendEmbed;
use crate::sources::youtube::youtube_client::{SearchError, YoutubeClient};
use dotenv::var;
use poise::serenity_prelude;
use serenity::all::audit_log::Action;
use serenity::all::{ChannelId, FullEvent, GatewayIntents, GuildChannel, GuildId, MemberAction};
use songbird::SerenityInit;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, RwLockWriteGuard};

pub struct MusicBotData {
    pub request_client: reqwest::Client,
    pub youtube_client: YoutubeClient,
    // pub spotify_client: SpotifyClient,
    pub database_pool: Arc<Database>,
    pub player: Arc<RwLock<Player>>,
    pub notifier: Arc<RwLock<Notifier>>
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

        let discord_token = var("DISCORD_TOKEN")
            .expect("Expected a valid discord token set in the configuration.");

        let database_url = var("DATABASE_URL")
            .expect("Expected a valid database url set in the configuration.");

        let framework = poise::Framework::<MusicBotData, MusicBotError>::builder()
            .options(poise::FrameworkOptions {
                on_error: |err| Box::pin(error_handler::handle(err)),
                commands: vec![
                    commands::cmd_help::help(),
                    music::cmd_play::play(),
                    music::cmd_skip::skip(),
                    music::cmd_stop::stop(),
                    music::cmd_vol::volume(),
                    music::cmd_join::join(),
                    music::cmd_queue::queue(),
                    music::cmd_leave::leave(),
                    music::cmd_shuffle::shuffle(),
                    music::cmd_playing::playing(),
                    utility::cmd_uwu::uwu(),
                    utility::cmd_uwu::uwu_me(),
                    utility::cmd_notify::notify(),
                    utility::cmd_wakeup::wakeup(),
                    utility::cmd_wakeup::wakeup_context(),
                ],
                pre_command: |ctx| Box::pin(async move {
                    println!("CMD: {} is executing {} ({})", ctx.author().name, ctx.command().name, ctx.invocation_string());
                }),
                event_handler: |ctx, event, a, b| Box::pin(async move {
                    match event {
                        // TODO: Move this somewhere else
                        FullEvent::GuildAuditLogEntryCreate { entry, guild_id } => {
                            match entry.action {
                                Action::Member(MemberAction::MemberDisconnect) => {
                                    if entry.user_id == a.bot_id {
                                        return Ok(());
                                    }

                                    guild_id.disconnect_member(ctx.http.clone(), entry.user_id).await?;

                                    let music_channel_id: ChannelId = ChannelId::new(829704972122718268);
                                    let guild_channels: HashMap<ChannelId, GuildChannel> = guild_id.channels(ctx.http.clone()).await?;

                                    let target_channel: Option<(&ChannelId, &GuildChannel)> = guild_channels
                                        .iter()
                                        .find(|(c, _): &(&ChannelId, &GuildChannel) | **c == music_channel_id);

                                    if let Some((_, guild_channel)) = target_channel {
                                        BotEmbed::YouShallNotKickMe
                                            .to_embed()
                                            .send_channel(ctx.http.clone(), guild_channel, None, None)
                                            .await?;
                                    }
                                }

                                _ => {}
                            };
                        }
                        _ => {}
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

                    println!("Bot ready");
                    println!("- Logged in as {}", ready.user.name);

                    println!("- Registering commands in guild");
                    poise::builtins::register_in_guild(ctx, &fw.options().commands, ready.guilds[0].id, )
                        .await
                        .map_err(|e| {
                            println!("Failed to register commands in guild. Error: {:?}", e);
                            MusicBotError::InternalError(e.to_string())
                        })?;

                    println!("- Connecting to database");
                    let database: Arc<Database> = Arc::new(
                        SqlitePoolOptions::new()
                            .connect(&database_url)
                            .await
                            .map_err(|e| {
                                println!("Failed to connect to database. Error: {:?}", e);
                                MusicBotError::InternalError(e.to_string())
                            })?
                    );

                    // Insert guild into database if it doesn't exist
                    let _ = sqlx::query!(
                        "INSERT OR IGNORE INTO guilds (guild_id, volume) VALUES ($1, $2)",
                        guild_id_map, 0.5
                    ).execute(&*database)
                        .await
                        .map_err(|e| {
                            println!("Failed to insert guild into database. Error: {:?}", e);
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
                        // spotify_client: SpotifyClient::new(),
                        database_pool: database,
                        player: player_handle,
                        notifier: notifier_handle
                    })
                })
            })
            .build();

        let serenity_client = serenity_prelude::Client::builder(discord_token, intents)
            .register_songbird()
            .framework(framework)
            .await
            .expect("Failed to build serenity client.");

        Self {
            serenity_client
        }
    }

    pub async fn start(&mut self) -> Result<(), MusicBotError> {
        println!("- Starting bot client");

        self.serenity_client.start().await
            .map_err(|e| {
                println!("- Failed to start server. Error: {:?}", e);
                MusicBotError::InternalError(e.to_string())
            })
    }
}