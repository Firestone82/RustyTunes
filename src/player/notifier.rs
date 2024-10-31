use std::sync::Arc;
use serenity::all::{ChannelId, GuildId, MessageId, UserId};
use serenity::futures::stream::Collect;
use sqlx::types::time::OffsetDateTime;
use crate::bot::{Context, Database};

pub struct MessageNotify {
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: UserId,
    message_id: MessageId,
    created_at: OffsetDateTime,
    notify_at: OffsetDateTime
}

pub struct Notifier {
    messages: Vec<MessageNotify>,
    database: Arc<Database>
}

impl Notifier {
    pub async fn new(ctx: serenity::prelude::Context, database: Arc<Database>) -> Self {
        // Load all messages
        let messages = sqlx::query!(
            "SELECT * FROM notify_me"
        ).fetch_all(&*database)
            .await
            .expect("Failed to fetch all messages from database");

        // let messages: Vec<MessageNotify> = messages.iter().map(|message| {
        //     message.message_id
        // // //     MessageNotify {
        // // //         guild_id: GuildId(message.guild_id),
        // // //         channel_id: ChannelId(message.channel_id),
        // // //         user_id: UserId(message.user_id),
        // // //         message_id: MessageId::new(message.message_id),
        // // //         created_at: message.created_at.unwrap(),
        // // //         notify_at: message.notify_at.unwrap()
        // // //     }
        // }).collect();

        Notifier {
            messages: Vec::new(),
            database
        }
    }
}