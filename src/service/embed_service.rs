use crate::bot::{Context, MusicBotError};
use serenity::all::{ChannelId, Color, CreateEmbed, CreateMessage, GuildChannel, Http, Message, MessageId};
use std::sync::Arc;

pub fn create_embed(
    color: Color,
    title: &str,
    description: &str,
) -> CreateEmbed {
    CreateEmbed::default()
        .color(color)
        .title(title)
        .description(description)
}

pub async fn send_channel_embed(
    http: Arc<Http>,
    channel: &GuildChannel,
    embed: CreateEmbed,
    delete_after: Option<u64>,
    message: Option<String>,
) -> Result<Message, MusicBotError> {
    send_channel_id_embed(http, channel.id, embed, delete_after, message).await
}

/// Same as `send_channel_embed` but targets a raw `ChannelId`. Used when we
/// need to write into a channel we don't have a cached `GuildChannel` for —
/// e.g. the voice channel's integrated text chat, looked up through the
/// bot's current voice state.
pub async fn send_channel_id_embed(
    http: Arc<Http>,
    channel_id: ChannelId,
    embed: CreateEmbed,
    delete_after: Option<u64>,
    message: Option<String>,
) -> Result<Message, MusicBotError> {
    let created_message = CreateMessage::default()
        .content(message.unwrap_or_default())
        .embed(embed);

    let message = channel_id
        .send_message(http.clone(), created_message)
        .await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    process_message(http, &message, delete_after).await;

    Ok(message)
}

pub async fn send_context_embed(
    ctx: Context<'_>,
    embed: CreateEmbed,
    reply: bool,
    delete_after: Option<u64>,
) -> Result<Message, MusicBotError> {
    let created_reply = poise::CreateReply::default().embed(embed).reply(reply);

    let reply_handle = ctx
        .send(created_reply)
        .await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    let message = reply_handle
        .into_message()
        .await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    let http = ctx.serenity_context().http.clone();
    process_message(http, &message, delete_after).await;

    Ok(message)
}

async fn process_message(
    http: Arc<Http>,
    message: &Message,
    delete_after: Option<u64>,
) {
    let channel_id: ChannelId = message.channel_id;
    let message_id: MessageId = message.id;

    if let Some(seconds) = delete_after {
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
            let _ = http
                .delete_message(channel_id, message_id, Some("Cleaning up last message"))
                .await;
        });
    }
}

/// Send a `CreateEmbed` via either a poise `Context` or a raw `GuildChannel`,
/// with optional auto-delete after N seconds.
pub trait SendEmbed {
    fn send_context(
        &self,
        ctx: Context<'_>,
        reply: bool,
        delete_after: Option<u64>,
    ) -> impl std::future::Future<Output = Result<Message, MusicBotError>> + Send;

    fn send_channel(
        &self,
        http: Arc<Http>,
        channel: &GuildChannel,
        delete_after: Option<u64>,
        message: Option<String>,
    ) -> impl std::future::Future<Output = Result<Message, MusicBotError>> + Send;

    fn send_channel_id(
        &self,
        http: Arc<Http>,
        channel_id: ChannelId,
        delete_after: Option<u64>,
        message: Option<String>,
    ) -> impl std::future::Future<Output = Result<Message, MusicBotError>> + Send;
}

impl SendEmbed for CreateEmbed {
    async fn send_context(
        &self,
        ctx: Context<'_>,
        reply: bool,
        delete_after: Option<u64>,
    ) -> Result<Message, MusicBotError> {
        let message: Message = send_context_embed(ctx, self.clone(), reply, delete_after).await?;
        Ok(message)
    }

    async fn send_channel(
        &self,
        http: Arc<Http>,
        channel: &GuildChannel,
        delete_after: Option<u64>,
        message: Option<String>,
    ) -> Result<Message, MusicBotError> {
        let message: Message = send_channel_embed(http, channel, self.clone(), delete_after, message).await?;
        Ok(message)
    }

    async fn send_channel_id(
        &self,
        http: Arc<Http>,
        channel_id: ChannelId,
        delete_after: Option<u64>,
        message: Option<String>,
    ) -> Result<Message, MusicBotError> {
        let message: Message = send_channel_id_embed(http, channel_id, self.clone(), delete_after, message).await?;
        Ok(message)
    }
}
