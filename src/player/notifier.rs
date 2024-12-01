use crate::bot::{Context, Database};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::service::embed_service::SendEmbed;
use regex::Regex;
use serenity::all::{ChannelId, GuildChannel, GuildId, Mentionable, Message, MessageId, UserId};
use sqlx::types::chrono;
use sqlx::types::time::OffsetDateTime;
use std::ops::Add;
use std::sync::Arc;
use std::time::Duration;
use time::{Date, PrimitiveDateTime, Time, UtcOffset};

#[derive(Debug, thiserror::Error)]
pub enum NotifierError {
    #[error("Whoops, an internal error occurred: {0}")]
    InternalError(String),

    #[error("Invalid time format")]
    InvalidTimeFormat
}

#[derive(Debug, Clone)]
pub struct MessageNotify {
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub message_id: MessageId,
    pub created_at: OffsetDateTime,
    pub notify_at: OffsetDateTime,
    pub note: Option<String>
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
            "SELECT *, 'b' as REE FROM notify_me"
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
                notify_at: message.notify_at.unwrap(),
                note: message.note.clone()
            }
        }).collect();

        Notifier {
            messages,
            database,
            serenity_context
        }
    }

    pub async fn add_message<'a>(&mut self, ctx: Context<'a>, notify_at: OffsetDateTime, note: Option<String>) -> Result<MessageNotify, NotifierError> {
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
            created_at: get_current_time(),
            notify_at,
            note
        };

        let guild_id: i64 = notify.guild_id.get() as i64;
        let channel_id: i64 = notify.channel_id.get() as i64;
        let user_id: i64 = notify.user_id.get() as i64;
        let message_id: i64 = notify.message_id.get() as i64;
        let current_time: OffsetDateTime = get_current_time();
        let note: Option<String> = notify.note.clone();

        sqlx::query!(
            "INSERT INTO notify_me (guild_id, channel_id, user_id, message_id, created_at, notify_at, note) VALUES (?, ?, ?, ?, ?, ?, ?)",
            guild_id, channel_id, user_id, message_id, current_time, notify_at, note
        ).execute(&*self.database)
            .await
            .expect("Failed to insert message into database");

        self.messages.push(notify.clone());

        Ok(notify)
    }

    pub async fn check_messages(&mut self) {
        for message in self.messages.clone().iter() {
            if message.notify_at <= get_current_time() {
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

pub fn parse_text(text: String) -> Result<OffsetDateTime, NotifierError> {
    let time = convert_literal_from_string(text.clone())
        .or_else(|| convert_time_date_from_string(text.clone()))
        .or_else(|| convert_time_offset_from_string(text.clone()));

    if let Some(time) = time {
        return Ok(time);
    }

    Err(NotifierError::InvalidTimeFormat)
}

pub fn convert_literal_from_string(text: String) -> Option<OffsetDateTime> {
    let re: Regex = Regex::new(r"^(week|tomorrow)$").unwrap();
    let mut offset: OffsetDateTime = get_current_time();

    if let Some(captures) = re.captures(&*text) {
        let capture = captures.get(1).map_or("", |m| m.as_str());

        match capture {
            "tomorrow" => {
                offset = offset.add(Duration::from_secs(24 * 60 * 60));
            }

            "week" => {
                offset = offset.add(Duration::from_secs(7 * 24 * 60 * 60));
            },

            _ => {}
        }

        return Some(offset);
    }

    None
}

// Function that returns fucking time, cuz rust is dum..
pub fn get_current_time() -> OffsetDateTime {
    let now_utc = OffsetDateTime::now_utc();
    let current_mont = now_utc.month() as u8;

    // Determine if the current time is during DST (CEST)
    let prague_offset = if current_mont >= 3 && current_mont <= 10 {
        UtcOffset::from_whole_seconds(7200).unwrap() // UTC +2
    } else {
        UtcOffset::from_whole_seconds(3600).unwrap() // UTC +1
    };

    now_utc.to_offset(prague_offset)
}

pub fn convert_time_date_from_string(text: String) -> Option<OffsetDateTime> {
    let date_format = time::format_description::parse("[day]-[month]-[year]").unwrap();
    if let Ok(date) = Date::parse(&text, &date_format) {
        let naive_datetime = PrimitiveDateTime::new(date, Time::from_hms(9, 0, 0).unwrap());
        return Some(naive_datetime.assume_utc());
    }

    let datetime_format = time::format_description::parse("[day]-[month]-[year]_[hour]:[minute]").unwrap();
    if let Ok(datetime) = PrimitiveDateTime::parse(&text, &datetime_format) {
        return Some(datetime.assume_utc());
    }

    None
}

pub fn convert_time_offset_from_string(text: String) -> Option<OffsetDateTime> {
    let re: Regex = Regex::new(r"^(?:(\d+)mo(?:nths?)?)?(?:(\d+)\s*d(?:ays?)?)?(?:(\d+)\s*h(?:ours?)?)?(?:(\d+)\s*m(?:inutes?)?)?(?:(\d+)\s*s(?:econds?)?)?$").unwrap();
    let mut offset: OffsetDateTime = get_current_time();

    if let Some(captures) = re.captures(&*text) {
        if let Some(months) = captures.get(1) {
            let months: u64 = months.as_str().parse().unwrap_or(0);
            offset = offset.add(Duration::from_secs(months * 30 * 24 * 3600));
        }

        if let Some(days) = captures.get(2) {
            let days: u64 = days.as_str().parse().unwrap_or(0);
            offset = offset.add(Duration::from_secs(days * 24 * 3600));
        }

        if let Some(hours) = captures.get(3) {
            let hours: u64 = hours.as_str().parse().unwrap_or(0);
            offset = offset.add(Duration::from_secs(hours * 3600));
        }

        if let Some(minutes) = captures.get(4) {
            let minutes: u64 = minutes.as_str().parse().unwrap_or(0);
            offset = offset.add(Duration::from_secs(minutes * 60));
        }

        if let Some(seconds) = captures.get(5) {
            let seconds: u64 = seconds.as_str().parse().unwrap_or(0);
            offset = offset.add(Duration::from_secs(seconds));
        }

        return Some(offset);
    }

    None
}

pub fn format_time(offset_date_time: OffsetDateTime) -> String {
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