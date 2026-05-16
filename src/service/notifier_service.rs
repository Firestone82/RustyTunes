use crate::bot::{Context, Database};
use crate::embeds::utility::notify_embeds::NotifyEmbed;
use crate::utils::time_utils::{get_current_time, TimeParseError};
use serenity::all::{ChannelId, CreateMessage, GuildChannel, GuildId, Mentionable, MessageId, MessageReference, UserId};
use sqlx::types::time::OffsetDateTime;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum NotifierError {
    // Bare display string — `MusicBotError::InternalError` already adds the
    // user-facing "Whoops…" prefix when this is converted at the boundary.
    #[error("{0}")]
    InternalError(String),

    #[error("Invalid time format")]
    InvalidTimeFormat,

    #[error("Notification not found")]
    NotFound,
}

impl From<TimeParseError> for NotifierError {
    fn from(_: TimeParseError) -> Self {
        NotifierError::InvalidTimeFormat
    }
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

#[derive(Debug, sqlx::FromRow)]
struct MessageNotifyRow {
    id: i64,
    guild_id: i64,
    channel_id: i64,
    user_id: i64,
    message_id: Option<i64>,
    created_at: OffsetDateTime,
    notify_at: OffsetDateTime,
    note: Option<String>,
}

impl From<MessageNotifyRow> for MessageNotify {
    fn from(r: MessageNotifyRow) -> Self {
        MessageNotify {
            id: r.id,
            guild_id: GuildId::new(r.guild_id as u64),
            channel_id: ChannelId::new(r.channel_id as u64),
            user_id: UserId::new(r.user_id as u64),
            message_id: r.message_id.map(|m| MessageId::new(m as u64)),
            created_at: r.created_at,
            notify_at: r.notify_at,
            note: r.note,
        }
    }
}

impl MessageNotify {
    /// Returns the note as it should be shown to a human, with any internal
    /// `[F:…]` / `[T:…]` metadata prefixes stripped.
    pub fn display_note(&self) -> Option<String> {
        let raw = self.note.as_deref()?;
        let (_, _, clean) = extract_metadata(raw);
        if clean.is_empty() {
            None
        } else {
            Some(clean)
        }
    }

    /// Extra users to ping when this notification fires (used by `/notify you`).
    /// Empty for plain `/notify me` entries.
    pub fn targets(&self) -> Vec<UserId> {
        match self.note.as_deref() {
            Some(raw) => extract_metadata(raw).1,
            None => Vec::new(),
        }
    }

    /// The user who scheduled this reminder, when it was scheduled *for*
    /// someone else via `/notify you`. `None` for self-scheduled `/notify me`
    /// entries (where the owner is the requester).
    pub fn scheduled_by(&self) -> Option<UserId> {
        match self.note.as_deref() {
            Some(raw) => extract_metadata(raw).0,
            None => None,
        }
    }
}

/// Encode the per-reminder metadata into the note as `[F:scheduler][T:t1,t2]rest`.
/// Either segment is optional — a plain `/notify me` entry stores just the raw note.
pub fn encode_metadata(
    scheduled_by: Option<UserId>,
    targets: &[UserId],
    note: &str,
) -> String {
    let mut prefix = String::new();
    if let Some(by) = scheduled_by {
        prefix.push_str(&format!("[F:{}]", by.get()));
    }
    if !targets.is_empty() {
        let ids: Vec<String> = targets.iter().map(|u| u.get().to_string()).collect();
        prefix.push_str(&format!("[T:{}]", ids.join(",")));
    }
    if prefix.is_empty() {
        note.to_string()
    } else {
        format!("{}{}", prefix, note)
    }
}

pub fn extract_metadata(note: &str) -> (Option<UserId>, Vec<UserId>, String) {
    let mut rest = note;

    let scheduled_by = if let Some(after_f) = rest.strip_prefix("[F:") {
        if let Some(end) = after_f.find(']') {
            let id = after_f[..end].parse::<u64>().ok().map(UserId::new);
            rest = &after_f[end + 1..];
            id
        } else {
            None
        }
    } else {
        None
    };

    let targets = if let Some(after_t) = rest.strip_prefix("[T:") {
        if let Some(end) = after_t.find(']') {
            let ids: Vec<UserId> = after_t[..end]
                .split(',')
                .filter_map(|s| s.parse::<u64>().ok())
                .map(UserId::new)
                .collect();
            rest = &after_t[end + 1..];
            ids
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    (scheduled_by, targets, rest.to_string())
}

pub struct Notifier {
    pub messages: Vec<MessageNotify>,
    database: Arc<Database>,
    serenity_context: serenity::prelude::Context,
}

impl Notifier {
    pub async fn new(
        serenity_context: serenity::prelude::Context,
        database: Arc<Database>,
    ) -> Self {
        let rows: Vec<MessageNotifyRow> = sqlx::query_as(
            "
            SELECT id, guild_id, channel_id, user_id, message_id, created_at, notify_at, note
            FROM notify_me
            ",
        )
        .fetch_all(&*database)
        .await
        .expect("Failed to fetch all messages from database");

        let messages: Vec<MessageNotify> = rows.into_iter().map(MessageNotify::from).collect();

        Notifier { messages, database, serenity_context }
    }

    pub async fn add_message(
        &mut self,
        ctx: Context<'_>,
        notify_at: OffsetDateTime,
        note: Option<String>,
    ) -> Result<MessageNotify, NotifierError> {
        let guild_id = ctx
            .guild_id()
            .ok_or_else(|| NotifierError::InternalError("Notify is only available in guilds".to_string()))?;

        let source_message_id: Option<MessageId> = match ctx {
            Context::Prefix(prefix) => Some(prefix.msg.id),
            _ => None,
        };

        self.add_message_for_user(
            guild_id,
            ctx.channel_id(),
            ctx.author().id,
            source_message_id,
            notify_at,
            note,
        )
        .await
    }

    pub async fn add_message_for_user(
        &mut self,
        guild_id: GuildId,
        channel_id: ChannelId,
        user_id: UserId,
        source_message_id: Option<MessageId>,
        notify_at: OffsetDateTime,
        note: Option<String>,
    ) -> Result<MessageNotify, NotifierError> {
        let guild_id_db: i64 = guild_id.get() as i64;
        let channel_id_db: i64 = channel_id.get() as i64;
        let user_id_db: i64 = user_id.get() as i64;
        let message_id_db: Option<i64> = source_message_id.map(|m| m.get() as i64);
        let created_at = get_current_time();

        let id = sqlx::query(
            "
            INSERT INTO notify_me (guild_id, channel_id, user_id, message_id, created_at, notify_at, note)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ",
        )
        .bind(guild_id_db)
        .bind(channel_id_db)
        .bind(user_id_db)
        .bind(message_id_db)
        .bind(created_at)
        .bind(notify_at)
        .bind(&note)
        .execute(&*self.database)
        .await
        .map_err(|e| NotifierError::InternalError(format!("DB insert failed: {e}")))?
        .last_insert_rowid();

        let notify = MessageNotify {
            id,
            guild_id,
            channel_id,
            user_id,
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
        // Allow removal by either the owner (the target the reminder fires for)
        // or the user who originally scheduled it via `/notify you`.
        let position = self
            .messages
            .iter()
            .position(|m| m.id == id && m.guild_id == guild_id && (m.user_id == user_id || m.scheduled_by() == Some(user_id)))
            .ok_or(NotifierError::NotFound)?;

        let removed = self.messages.remove(position);

        sqlx::query("DELETE FROM notify_me WHERE id = ?")
            .bind(id)
            .execute(&*self.database)
            .await
            .map_err(|e| NotifierError::InternalError(format!("DB delete failed: {e}")))?;

        Ok(removed)
    }

    pub fn list_for_user(
        &self,
        user_id: UserId,
        guild_id: GuildId,
    ) -> Vec<MessageNotify> {
        let mut out: Vec<MessageNotify> = self
            .messages
            .iter()
            .filter(|m| m.user_id == user_id && m.guild_id == guild_id)
            .cloned()
            .collect();
        out.sort_by_key(|m| m.notify_at);
        out
    }

    pub async fn check_messages(&mut self) {
        let now = get_current_time();
        let due: Vec<MessageNotify> = self
            .messages
            .iter()
            .filter(|m| m.notify_at <= now)
            .cloned()
            .collect();

        for message in due {
            let guild_channel: GuildChannel = match self
                .serenity_context
                .http
                .get_channel(message.channel_id)
                .await
            {
                Ok(ch) => match ch.guild() {
                    Some(gc) => gc,
                    None => {
                        tracing::error!(
                            "Notification channel {} is not a guild channel",
                            message.channel_id
                        );
                        self.drop_notification(message.id).await;
                        continue;
                    }
                },
                Err(e) => {
                    tracing::error!(
                        "Failed to fetch notification channel {}: {:?}",
                        message.channel_id,
                        e
                    );
                    self.drop_notification(message.id).await;
                    continue;
                }
            };

            let targets = message.targets();
            let content = if targets.is_empty() {
                format!("||{}||", message.user_id.mention())
            } else {
                targets
                    .iter()
                    .map(|u| u.mention().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            };

            let embed = NotifyEmbed::Notification(&message).to_embed();
            let mut create_message = CreateMessage::default().content(content).embed(embed);

            if let Some(reply_to) = message.message_id {
                let reference = MessageReference::from((message.channel_id, reply_to));
                create_message = create_message.reference_message(reference);
            }

            let send_result = guild_channel
                .send_message(self.serenity_context.http.clone(), create_message)
                .await
                .map_err(|e| crate::bot::MusicBotError::InternalError(e.to_string()));

            if let Err(e) = send_result {
                tracing::error!("Failed to send notification {}: {:?}", message.id, e);
            }

            self.drop_notification(message.id).await;
        }
    }

    async fn drop_notification(
        &mut self,
        id: i64,
    ) {
        let _ = sqlx::query("DELETE FROM notify_me WHERE id = ?")
            .bind(id)
            .execute(&*self.database)
            .await
            .map_err(|e| tracing::error!("Failed to delete notification {}: {:?}", id, e));

        self.messages.retain(|m| m.id != id);
    }
}
