use crate::bot::commands;
use crate::bot::player::playback::Playback;
use crate::bot::youtube::client::YoutubeClient;
use dotenv::var;
use poise::serenity_prelude;
use serenity::all::GuildId;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tokio::sync::RwLock;

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

pub struct MusicBotData {
    pub request_client: reqwest::Client,
    pub youtube_client: YoutubeClient,
    pub playback: Arc<RwLock<Playback>>,
    pub database: Arc<RwLock<sqlx::SqlitePool>>
}

pub struct MusicBotClient {
    serenity_client: serenity_prelude::Client,
}

impl MusicBotClient {
    pub async fn new() -> Self {
        use poise::serenity_prelude::GatewayIntents;
        use songbird::SerenityInit;

        let intents = GatewayIntents::non_privileged()
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_VOICE_STATES
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_PRESENCES;

        let discord_token = var("DISCORD_TOKEN").expect("Expected a token in the environment.");
        let production: bool = var("PRODUCTION").expect("Expected a boolean in the environment.") == "true";

        let database: Pool<Sqlite> = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename("database.sqlite")
                    .create_if_missing(true),
            )
            .await
            .expect("Couldn't connect to database");

        sqlx::migrate!("./migrations")
            .run(&database)
            .await
            .expect("Couldn't run database migrations");

        let framework = poise::Framework::<MusicBotData, MusicBotError>::builder()
            .options(poise::FrameworkOptions {
                on_error: |err| Box::pin(Self::handle_error(err)),
                commands: vec![
                    commands::cmd_help::help(),
                    commands::cmd_play::play(),
                    commands::cmd_search::search(),
                    commands::cmd_stop::stop(),
                    commands::cmd_skip::skip(),
                    commands::cmd_vol::vol(),
                ],
                prefix_options: poise::PrefixFrameworkOptions {
                    prefix: Some(String::from("!")),
                    ..Default::default()
                },
                ..Default::default()
            })
            .setup(move |ctx, ready, fw| {
                Box::pin(async move {
                    println!("Logged in as {}", ready.user.name);
                    let guild_id: GuildId = ready.guilds[0].id;

                    let _ = if !production {
                        println!("- Registering commands in guild");

                        poise::builtins::register_in_guild(
                            &ctx.http,
                            &fw.options().commands,
                            guild_id,
                        ).await
                    } else {
                        poise::builtins::register_globally(ctx, &fw.options().commands).await
                    };

                    Ok(MusicBotData {
                        request_client: reqwest::Client::new(),
                        youtube_client: YoutubeClient::new(),
                        playback:  Arc::new(RwLock::new(Playback::default())),
                        database: Arc::new(RwLock::new(database))
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
        self.serenity_client.start().await.map_err(|e| {
            println!("Failed to start server. Error: {:?}", e);
            MusicBotError::InternalError(e.to_string())
        })
    }

    async fn handle_error(error: poise::FrameworkError<'_, MusicBotData, MusicBotError>) {
        match error {
            // Bot failed to start
            poise::FrameworkError::Setup { error, .. } => {
                panic!("Failed to start bot: {:?}", error)
            },

            // Command failed to execute
            poise::FrameworkError::Command { error, ctx, .. } => {
                println!("Error in command `{}`: {:?}", ctx.command().name, error,);
                let _ = ctx.reply(error.to_string()).await;
            }

            // Command check failed
            poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
                if let Some(error) = error {
                    let _ = ctx.reply(error.to_string()).await;
                }
            }

            // Unmatched errors
            error => {
                if let Err(e) = poise::builtins::on_error(error).await {
                    println!("Error while handling error: {}", e)
                }
            }

        }
    }

}