#![warn(clippy::str_to_string)]

use crate::bot::client::{MusicBotClient, MusicBotError};

pub mod bot;

#[tokio::main]
async fn main() -> Result<(), MusicBotError> {

    println!("Starting server.");
    let _ = MusicBotClient::new()
        .await
        .start()
        .await?;

    let _ = tokio::signal::ctrl_c().await;
    println!("Received Ctrl-C, shutting down.");

    Ok(())
}