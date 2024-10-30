use crate::bot::{Context, MusicBotError};
use serenity::all::{Color, CreateEmbed, CreateMessage, GuildChannel, Http, Message};
use std::sync::Arc;

/*
 * Send embeds
 */

pub fn create_embed(color: Color, title: &str, description: &str) -> CreateEmbed {
    CreateEmbed::default()
        .color(color)
        .title(title)
        .description(description)
}

pub async fn send_channel_embed(http: Arc<Http>, channel: &GuildChannel, embed: CreateEmbed, delete_after: Option<u64>) -> Result<Message, MusicBotError> {
    let created_message = CreateMessage::default()
        .embed(embed);

    let message = channel.send_message(http.clone(), created_message).await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    process_message(http, &message, delete_after).await;

    Ok(message)
}

pub async fn send_context_embed(ctx: Context<'_>, embed: CreateEmbed, reply: bool, delete_after: Option<u64>) -> Result<Message, MusicBotError> {
    let created_reply = poise::CreateReply::default()
        .embed(embed)
        .reply(reply);

    let reply_handle = ctx.send(created_reply).await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    let message = reply_handle.into_message().await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    let http = ctx.serenity_context().http.clone();
    process_message(http, &message, delete_after).await;

    Ok(message)
}

async fn process_message(http: Arc<Http>, message: &Message, delete_after: Option<u64>) {
    let channel_id = message.channel_id.clone();
    let message_id = message.id.clone();

    if let Some(seconds) = delete_after {
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
            let _ = http.delete_message(channel_id,message_id, Some("Cleaning up last message")).await;
        });
    }
}

// Implement send method for CreateEmbed
pub trait SendEmbed {
    fn send_context(&self, ctx: Context<'_>, reply: bool, delete_after: Option<u64>) 
        -> impl std::future::Future<Output = Result<Message, MusicBotError>> + Send;
    
    fn send_channel(&self, http: Arc<Http>, channel: &GuildChannel, delete_after: Option<u64>) 
        -> impl std::future::Future<Output = Result<Message, MusicBotError>> + Send;
}

impl SendEmbed for CreateEmbed {
    async fn send_context(&self, ctx: Context<'_>, reply: bool, delete_after: Option<u64>) -> Result<Message, MusicBotError> {
        let message: Message = send_context_embed(ctx, self.clone(), reply, delete_after).await?;
        Ok(message)
    }
    
    async fn send_channel(&self, http: Arc<Http>, channel: &GuildChannel, delete_after: Option<u64>) -> Result<Message, MusicBotError> {
        let message: Message = send_channel_embed(http, channel, self.clone(), delete_after).await?;
        Ok(message)
    }
}