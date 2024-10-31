use crate::bot::{Context, Database};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::service::embed_service::SendEmbed;
use regex::Regex;
use serenity::all::{ChannelId, GuildChannel, GuildId, Mentionable, Message, MessageId, UserId};
use sqlx::types::time::OffsetDateTime;
use std::ops::Add;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum NotifierError {
    #[error("Whoops, an internal error occurred: {0}")]
    InternalError(String),

    #[error("Invalid time format")]
    InvalidTimeFormat
}

#[derive(Debug, Clone, Copy)]
pub struct MessageNotify {
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub message_id: MessageId,
    pub created_at: OffsetDateTime,
    pub notify_at: OffsetDateTime
}

pub struct Notifier {
    pub messages: Vec<MessageNotify>,
    database: Arc<Database>,
    serenity_context: serenity::prelude::Context
}

impl Notifier {
    pub async fn new(serenity_context: serenity::prelude::Context, database: Arc<Database>) -> Self {
        // Sqlx didn't want to refresh. So I was forced to add a random column to the query.
        let messages = sqlx::query!(
            "SELECT *, 'a' as REE FROM notify_me" 
        ).fetch_all(&*database)
            .await
            .expect("Failed to fetch all messages from database");

        let messages: Vec<MessageNotify> = messages.iter().map(|message| {
            MessageNotify {
                guild_id: GuildId::new(message.guild_id as u64),
                channel_id: ChannelId::new(message.channel_id as u64),
                user_id: UserId::new(message.user_id as u64),
                message_id: MessageId::new(message.message_id as u64),
                created_at: message.created_at.unwrap(),
                notify_at: message.notify_at.unwrap()
            }
        }).collect();

        Notifier {
            messages,
            database,
            serenity_context
        }
    }

    pub async fn add_message<'a>(&mut self, ctx: Context<'a>, notify_at: OffsetDateTime) -> Result<MessageNotify, NotifierError> {
        let msg: &Message = match ctx {
            Context::Prefix(ctx) => {
                ctx.msg
            }
            _ => {
                return Err(NotifierError::InternalError("Invalid context".to_string()));
            }
        };

        let notify: MessageNotify = MessageNotify {
            guild_id: ctx.guild_id().unwrap(),
            channel_id: ctx.channel_id(),
            user_id: ctx.author().id,
            message_id: msg.id,
            created_at: OffsetDateTime::now_utc(),
            notify_at
        };

        let guild_id: i64 = notify.guild_id.get() as i64;
        let channel_id: i64 = notify.channel_id.get() as i64;
        let user_id: i64 = notify.user_id.get() as i64;
        let message_id: i64 = notify.message_id.get() as i64;
        let current_time: OffsetDateTime = OffsetDateTime::now_local().unwrap();

        sqlx::query!(
            "INSERT INTO notify_me (guild_id, channel_id, user_id, message_id, created_at, notify_at) VALUES (?, ?, ?, ?, ?, ?)",
            guild_id, channel_id, user_id, message_id, current_time, notify_at
        ).execute(&*self.database)
            .await
            .expect("Failed to insert message into database");

        self.messages.push(notify);

        Ok(notify)
    }

    pub async fn check_messages(&mut self) {
        for message in self.messages.clone().iter() {
            if message.notify_at <= OffsetDateTime::now_local().unwrap() {
                let message_id: i64 = message.message_id.get() as i64;
                
                let guild_channel: GuildChannel = self.serenity_context.http.get_channel(message.channel_id)
                    .await
                    .expect("Failed to get channel")
                    .guild()
                    .expect("Failed to get guild channel");
                
                NotifyEmbed::Notification(message)
                    .to_embed()
                    .send_channel(self.serenity_context.http.clone(), &guild_channel, None, Some(format!("||{}||", message.user_id.mention())))
                    .await
                    .expect("Failed to send notification");

                let _ = sqlx::query!(
                    "DELETE FROM notify_me WHERE message_id = ?",
                    message_id
                ).execute(&*self.database)
                    .await
                    .expect("Failed to delete message from database");
                
                self.messages.retain(|m| m.message_id != message.message_id);
            }
        }
    }
}

pub fn convert_time_string(time: &str) -> Result<OffsetDateTime, NotifierError> {
    let re: Regex = Regex::new(r"(?:(\d+)\s*mo(?:nths?)?)?\s*(?:(\d+)\s*d(?:ays?)?)?\s*(?:(\d+)\s*h(?:ours?)?)?\s*(?:(\d+)\s*m(?:inutes?)?)?\s*(?:(\d+)\s*s(?:econds?)?)?")
        .map_err(|e| NotifierError::InternalError(e.to_string()))?;

    let mut seconds_to_add: i64 = 0;

    if let Some(captures) = re.captures(time) {
        let months: i64 = captures.get(1)
            .map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(0));
        seconds_to_add += months * 30 * 24 * 3600;

        let days: i64  = captures.get(2)
            .map_or(0, |d| d.as_str().parse::<i64>().unwrap_or(0));
        seconds_to_add += days * 24 * 3600;

        let hours: i64  = captures.get(3)
            .map_or(0, |h| h.as_str().parse::<i64>().unwrap_or(0));
        seconds_to_add += hours * 3600;

        let minutes: i64  = captures.get(4)
            .map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(0));
        seconds_to_add += minutes * 60;

        let seconds: i64  = captures.get(5)
            .map_or(0, |s| s.as_str().parse::<i64>().unwrap_or(0));
        seconds_to_add += seconds;

        Ok(OffsetDateTime::now_local().unwrap().add(Duration::from_secs(seconds_to_add as u64)))
    } else {
        Err(NotifierError::InvalidTimeFormat)
    }
}

pub fn format_time(offset_date_time: OffsetDateTime) -> String {
    // Format each component manually
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        offset_date_time.year(),
        offset_date_time.month() as u8,
        offset_date_time.day(),
        offset_date_time.hour(),
        offset_date_time.minute(),
        offset_date_time.second()
    )
}