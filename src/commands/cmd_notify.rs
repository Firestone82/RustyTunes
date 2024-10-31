use crate::bot::{Context, MusicBotError};
use crate::service::embed_service::SendEmbed;
use serenity::all::{Color, CreateEmbed};
use serenity::all::{Mention, Mentionable};
use uwuifier::uwuify_str_sse;

#[poise::command(
    prefix_command, slash_command,
)]
pub async fn notify_me(ctx: Context<'_>, time: Vec<String>) -> Result<(), MusicBotError> {

    Ok(())
}