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
use serenity::all::{ChannelId, FullEvent, GatewayIntents, GuildChannel, GuildId, MemberAction, Mentionable};
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
                    tracing::info!("CMD: {} is executing {} ({})", ctx.author().name, ctx.command().name, ctx.invocation_string());
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

                        let bot_channel = match bot_channel {
                            Some(c) => c,
                            None => return Ok(()),
                        };

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

                            if let Some(manager) = songbird::get(ctx).await {
                                let _ = manager.remove(guild_id).await;
                            }
                        }
                    }

                    Ok(())
                }),
                // TODO: Unable to receive targeted member of DisconnectEvent. :( Since revenge is not possible to make in current state.
                // event_handler_old: |ctx, event, a, _| Box::pin(async move {
                    // match event {
                        // TODO: Move this somewhere else
                        // FullEvent::GuildAuditLogEntryCreate { entry, guild_id } => {
                        //     match entry.action {
                        //         Action::Member(MemberAction::MemberDisconnect) => {
                        //             // Ignore if the one that's disconnecting is the bot
                        //             if entry.user_id == a.bot_id {
                        //                 return Ok(());
                        //             }
                        //             
                        //             // Ignore if the target is not the bot
                        //             if let Some(target) = entry.target_id {
                        //                 if target.get() != a.bot_id.get() {
                        //                     return Ok(());
                        //                 }
                        //             } else {
                        //                 return Ok(());
                        //             };
                        // 
                        //             println!("User {} disconnected me, taking revenge!", entry.user_id);
                        //             guild_id.disconnect_member(ctx.http.clone(), entry.user_id).await?;
                        // 
                        //             let music_channel_id: ChannelId = ChannelId::new(829704972122718268);
                        //             let guild_channels: HashMap<ChannelId, GuildChannel> = guild_id.channels(ctx.http.clone()).await?;
                        // 
                        //             let target_channel: Option<(&ChannelId, &GuildChannel)> = guild_channels
                        //                 .iter()
                        //                 .find(|(c, _): &(&ChannelId, &GuildChannel) | **c == music_channel_id);
                        //             
                        //             if let Some((_, guild_channel)) = target_channel {
                        //                 BotEmbed::YouShallNotKickMe
                        //                     .to_embed()
                        //                     .send_channel(ctx.http.clone(), guild_channel, None, Some(format!("{}", entry.user_id.mention())))
                        //                     .await?;
                        //             }
                        //         }
                        // 
                        //         _ => {}
                        //     };
                        // }
                        // _ => {}
                    // }
                    // 
                    // Ok(())
                // }),
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

                    tracing::info!("Registering commands in guild");
                    poise::builtins::register_in_guild(ctx, &fw.options().commands, ready.guilds[0].id, )
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
        tracing::info!("Starting bot client");

        self.serenity_client.start().await
            .map_err(|e| {
                tracing::error!("Failed to start server: {:?}", e);
                MusicBotError::InternalError(e.to_string())
            })
    }
}