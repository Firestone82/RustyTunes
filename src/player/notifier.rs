use crate::bot::{Context, Database};
use crate::embeds::notify_embeds::NotifyEmbed;
use crate::service::embed_service::SendEmbed;
use regex::Regex;
use serenity::all::{ChannelId, GuildChannel, GuildId, Mentionable, MessageId, UserId};
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
    InvalidTimeFormat,

    #[error("Notification not found")]
    NotFound,
}

#[derive(Debug, Clone)]
pub struct MessageNotify {
    pub id: i64,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub message_id: Option<MessageId>,
    pub created_at: OffsetDateTime,
    pub notify_at: OffsetDateTime,
    pub note: Option<String>,
}

pub struct Notifier {
    pub messages: Vec<MessageNotify>,
    database: Arc<Database>,
    serenity_context: serenity::prelude::Context,
}

impl Notifier {
    pub async fn new(serenity_context: serenity::prelude::Context, database: Arc<Database>) -> Self {
        let rows = sqlx::query!(
            "SELECT id, guild_id, channel_id, user_id, message_id, created_at, notify_at, note FROM notify_me"
        ).fetch_all(&*database)
            .await
            .expect("Failed to fetch all messages from database");

        let messages: Vec<MessageNotify> = rows.into_iter().map(|r| MessageNotify {
            id: r.id,
            guild_id: GuildId::new(r.guild_id as u64),
            channel_id: ChannelId::new(r.channel_id as u64),
            user_id: UserId::new(r.user_id as u64),
            message_id: r.message_id.map(|m| MessageId::new(m as u64)),
            created_at: r.created_at.unwrap(),
            notify_at: r.notify_at.unwrap(),
            note: r.note,
        }).collect();

        Notifier {
            messages,
            database,
            serenity_context,
        }
    }

    pub async fn add_message(
        &mut self,
        ctx: Context<'_>,
        notify_at: OffsetDateTime,
        note: Option<String>,
    ) -> Result<MessageNotify, NotifierError> {
        let guild_id = ctx.guild_id().ok_or_else(|| {
            NotifierError::InternalError("Notify is only available in guilds".to_string())
        })?;

        // Prefix invocations have a source message we can link back to;
        // slash invocations don't, so we store NULL.
        let source_message_id: Option<MessageId> = match ctx {
            Context::Prefix(prefix) => Some(prefix.msg.id),
            _ => None,
        };

        let guild_id_db: i64 = guild_id.get() as i64;
        let channel_id_db: i64 = ctx.channel_id().get() as i64;
        let user_id_db: i64 = ctx.author().id.get() as i64;
        let message_id_db: Option<i64> = source_message_id.map(|m| m.get() as i64);
        let created_at = get_current_time();

        let id = sqlx::query!(
            "INSERT INTO notify_me (guild_id, channel_id, user_id, message_id, created_at, notify_at, note) VALUES (?, ?, ?, ?, ?, ?, ?)",
            guild_id_db, channel_id_db, user_id_db, message_id_db, created_at, notify_at, note
        )
            .execute(&*self.database)
            .await
            .map_err(|e| NotifierError::InternalError(format!("DB insert failed: {e}")))?
            .last_insert_rowid();

        let notify = MessageNotify {
            id,
            guild_id,
            channel_id: ctx.channel_id(),
            user_id: ctx.author().id,
            message_id: source_message_id,
            created_at,
            notify_at,
            note,
        };

        self.messages.push(notify.clone());
        Ok(notify)
    }

    pub async fn remove_for_user(
        &mut self,
        user_id: UserId,
        guild_id: GuildId,
        id: i64,
    ) -> Result<MessageNotify, NotifierError> {
        let position = self.messages.iter().position(|m| {
            m.id == id && m.user_id == user_id && m.guild_id == guild_id
        }).ok_or(NotifierError::NotFound)?;

        let removed = self.messages.remove(position);

        sqlx::query!("DELETE FROM notify_me WHERE id = ?", id)
            .execute(&*self.database)
            .await
            .map_err(|e| NotifierError::InternalError(format!("DB delete failed: {e}")))?;

        Ok(removed)
    }

    pub fn list_for_user(&self, user_id: UserId, guild_id: GuildId) -> Vec<MessageNotify> {
        let mut out: Vec<MessageNotify> = self.messages
            .iter()
            .filter(|m| m.user_id == user_id && m.guild_id == guild_id)
            .cloned()
            .collect();
        out.sort_by_key(|m| m.notify_at);
        out
    }

    pub async fn check_messages(&mut self) {
        let now = get_current_time();
        let due: Vec<MessageNotify> = self.messages
            .iter()
            .filter(|m| m.notify_at <= now)
            .cloned()
            .collect();

        for message in due {
            let guild_channel: GuildChannel = match self.serenity_context.http.get_channel(message.channel_id).await {
                Ok(ch) => match ch.guild() {
                    Some(gc) => gc,
                    None => {
                        tracing::error!("Notification channel {} is not a guild channel", message.channel_id);
                        self.drop_notification(message.id).await;
                        continue;
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to fetch notification channel {}: {:?}", message.channel_id, e);
                    self.drop_notification(message.id).await;
                    continue;
                }
            };

            let send_result = NotifyEmbed::Notification(&message)
                .to_embed()
                .send_channel(
                    self.serenity_context.http.clone(),
                    &guild_channel,
                    None,
                    Some(format!("||{}||", message.user_id.mention())),
                )
                .await;

            if let Err(e) = send_result {
                tracing::error!("Failed to send notification {}: {:?}", message.id, e);
            }

            self.drop_notification(message.id).await;
        }
    }

    async fn drop_notification(&mut self, id: i64) {
        let _ = sqlx::query!("DELETE FROM notify_me WHERE id = ?", id)
            .execute(&*self.database)
            .await
            .map_err(|e| tracing::error!("Failed to delete notification {}: {:?}", id, e));

        self.messages.retain(|m| m.id != id);
    }
}

pub fn parse_text(text: String) -> Result<OffsetDateTime, NotifierError> {
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err(NotifierError::InvalidTimeFormat);
    }

    let time = convert_literal_from_string(trimmed.clone())
        .or_else(|| convert_time_date_from_string(trimmed.clone()))
        .or_else(|| convert_time_offset_from_string(trimmed.clone()));

    time.ok_or(NotifierError::InvalidTimeFormat)
}

pub fn convert_literal_from_string(text: String) -> Option<OffsetDateTime> {
    let now = get_current_time();
    match text.as_str() {
        "tomorrow" => Some(now.add(Duration::from_secs(24 * 60 * 60))),
        "week" => Some(now.add(Duration::from_secs(7 * 24 * 60 * 60))),
        _ => None,
    }
}

// Returns local offset, switching between CET (UTC+1) and CEST (UTC+2) by month.
pub fn get_current_time() -> OffsetDateTime {
    let now_utc: OffsetDateTime = OffsetDateTime::now_utc();
    let current_month: u8 = now_utc.month() as u8;

    let utc_offset: UtcOffset = if (3..=10).contains(&current_month) {
        UtcOffset::from_whole_seconds(7200).unwrap() // UTC+2
    } else {
        UtcOffset::from_whole_seconds(3600).unwrap() // UTC+1
    };

    now_utc.to_offset(utc_offset)
}

pub fn convert_time_date_from_string(text: String) -> Option<OffsetDateTime> {
    let local_offset = get_current_time().offset();

    let date_format = time::format_description::parse("[day]-[month]-[year]").unwrap();
    if let Ok(date) = Date::parse(&text, &date_format) {
        let naive = PrimitiveDateTime::new(date, Time::from_hms(9, 0, 0).unwrap());
        return Some(naive.assume_offset(local_offset));
    }

    let datetime_format = time::format_description::parse("[day]-[month]-[year]_[hour]:[minute]").unwrap();
    if let Ok(datetime) = PrimitiveDateTime::parse(&text, &datetime_format) {
        return Some(datetime.assume_offset(local_offset));
    }

    None
}

pub fn convert_time_offset_from_string(text: String) -> Option<OffsetDateTime> {
    let re: Regex = Regex::new(
        r"^(?:(\d+)mo(?:nths?)?)?\s*(?:(\d+)\s*d(?:ays?)?)?\s*(?:(\d+)\s*h(?:ours?)?)?\s*(?:(\d+)\s*m(?:inutes?)?)?\s*(?:(\d+)\s*s(?:econds?)?)?$"
    ).unwrap();

    let captures = re.captures(text.as_str())?;

    let mut total_secs: u64 = 0;
    let mut matched_any = false;

    let units = [
        (1u64, 30 * 24 * 3600),
        (2, 24 * 3600),
        (3, 3600),
        (4, 60),
        (5, 1),
    ];

    for (group, multiplier) in units {
        if let Some(m) = captures.get(group as usize) {
            let v: u64 = m.as_str().parse().unwrap_or(0);
            total_secs = total_secs.saturating_add(v.saturating_mul(multiplier));
            matched_any = true;
        }
    }

    if !matched_any || total_secs == 0 {
        return None;
    }

    Some(get_current_time().add(Duration::from_secs(total_secs)))
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
