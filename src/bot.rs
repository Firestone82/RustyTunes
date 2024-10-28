use crate::commands;
use crate::handlers::error_handler;
use crate::player::player::Player;
use crate::sources::youtube::youtube_client::YoutubeClient;
use dotenv::var;
use poise::serenity_prelude;
use serenity::all::GatewayIntents;
use songbird::SerenityInit;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MusicBotData {
    pub request_client: reqwest::Client,
    pub youtube_client: YoutubeClient,
    // pub spotify_client: SpotifyClient,
    pub database: Pool<Sqlite>,
    pub player: Arc<RwLock<Player>>,
}

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
                    commands::cmd_play::play(),
                    commands::cmd_skip::skip(),
                    commands::cmd_stop::stop(),
                    commands::cmd_vol::volume(),
                ],
                prefix_options: poise::PrefixFrameworkOptions {
                    prefix: Some(String::from("!")),
                    ..Default::default()
                },
                ..Default::default()
            })
            .setup(move |ctx, ready, fw| {
                Box::pin(async move {
                    let guild_id: i64 = ready.guilds[0].id.get() as i64;

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
                    let pool: Pool<Sqlite> = SqlitePoolOptions::new()
                        .connect(&database_url)
                        .await
                        .map_err(|e| {
                            println!("Failed to connect to database. Error: {:?}", e);
                            MusicBotError::InternalError(e.to_string())
                        })?;

                    // Insert guild into database if it doesn't exist
                    let _ = sqlx::query!(
                        "INSERT OR IGNORE INTO guilds (guild_id, volume) VALUES ($1, $2)",
                        guild_id, 0.5
                    ).execute(&pool)
                        .await
                        .map_err(|e| {
                            println!("Failed to insert guild into database. Error: {:?}", e);
                            MusicBotError::InternalError(e.to_string())
                        })?;

                    let guild_record = sqlx::query!(
                        "SELECT * FROM guilds WHERE guild_id = $1",
                        guild_id
                    ).fetch_one(&pool)
                        .await
                        .map_err(|e| {
                            println!("Failed to fetch volume from database. Error: {:?}", e);
                            MusicBotError::InternalError(e.to_string())
                        })?;

                    let mut player: Player = Player::default();
                    
                    if let Some(volume) = guild_record.volume {
                        player.set_volume(volume as f32)
                            .await
                            .map_err(|e| {
                                println!("Failed to set volume. Error: {:?}", e);
                                MusicBotError::InternalError(e.to_string())
                            })?;
                    }
                    
                    Ok(MusicBotData {
                        request_client: reqwest::Client::new(),
                        youtube_client: YoutubeClient::new(),
                        // spotify_client: SpotifyClient::new(),
                        database: pool,
                        player: Arc::new(RwLock::new(player)),
                    })
                })
            })
            .build();

        let serenity_client = poise::serenity_prelude::Client::builder(discord_token, intents)
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

    pub async fn stop(&mut self) -> Result<(), MusicBotError> {
        // TODO: Implement stop method
        Ok(())
    }
}