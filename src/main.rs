use crate::bot::{MusicBotClient, MusicBotError};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub mod bot;
pub mod player;
pub mod handlers;
pub mod commands;
pub mod sources;
pub mod checks;
pub mod service;
pub mod embeds;

#[tokio::main]
async fn main() -> Result<(), MusicBotError> {
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("rusttunes")
        .filename_suffix("log")
        .max_log_files(30)
        .build("logs")
        .expect("Failed to initialise rolling log file appender");

    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,rust_tunes=debug,RustTunes=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(false))
        .with(
            fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(file_writer),
        )
        .init();

    tracing::info!("Starting server.");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls ring CryptoProvider");

    MusicBotClient::new()
        .await
        .start()
        .await?;

    // TODO: Properly handle shutdown logic

    Ok(())
}
