use crate::bot::{MusicBotClient, MusicBotError};

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
    println!("Starting server.");

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