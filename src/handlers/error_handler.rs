use crate::bot::{MusicBotData, MusicBotError};
use crate::embeds::bot::bot_embeds::BotEmbed;
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use async_trait::async_trait;
use poise::serenity_prelude;
use serenity::all::GuildChannel;
use songbird::tracks::PlayMode;
use songbird::{Event, EventContext, EventHandler};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Delete the user's prefix-command invocation after 30s so the channel
/// doesn't get cluttered. Slash commands clean themselves up.
pub fn schedule_prefix_delete(ctx: poise::Context<'_, MusicBotData, MusicBotError>) {
    if let poise::Context::Prefix(prefix_ctx) = ctx {
        let http = ctx.serenity_context().http.clone();
        let channel_id = prefix_ctx.msg.channel_id;
        let message_id = prefix_ctx.msg.id;
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            let _ = http.delete_message(channel_id, message_id, None).await;
        });
    }
}

/// Songbird event handler for `TrackEvent::Error` — logs the failure and
/// surfaces it to the text channel so users see *why* a track was skipped
/// (typically a yt-dlp signature/extraction error). Songbird also fires
/// `TrackEvent::End` after an error, so the per-track `QueueHandler` keeps
/// the queue moving — we only handle the user-facing notice here.
pub struct ErrorHandler {
    serenity_ctx: serenity_prelude::Context,
    player: Arc<RwLock<Player>>,
    guild_channel: Option<GuildChannel>,
}

impl ErrorHandler {
    pub fn new(
        serenity_ctx: serenity_prelude::Context,
        player: Arc<RwLock<Player>>,
        guild_channel: Option<GuildChannel>,
    ) -> Self {
        Self { serenity_ctx, player, guild_channel }
    }
}

#[async_trait]
impl EventHandler for ErrorHandler {
    async fn act(
        &self,
        e: &EventContext<'_>,
    ) -> Option<Event> {
        let reason = match e {
            EventContext::Track(track_list) => track_list
                .iter()
                .find_map(|(state, _)| match &state.playing {
                    PlayMode::Errored(err) => Some(err.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| "unknown playback error".to_string()),
            _ => "unknown playback error".to_string(),
        };

        tracing::error!("Track error event: {}", reason);

        let title = self
            .player
            .read()
            .await
            .current_track
            .as_ref()
            .map(|t| t.metadata.title.clone());

        let description = match title {
            Some(t) => format!("Failed to play **{t}** — {reason}"),
            None => format!("Playback failed — {reason}"),
        };

        if let Some(channel) = &self.guild_channel {
            let _ = PlayerEmbed::PlaybackErrorEmbed(description)
                .to_embed()
                .send_channel(self.serenity_ctx.http.clone(), channel, Some(60), None)
                .await;
        }

        None
    }
}

pub async fn handle(error: poise::FrameworkError<'_, MusicBotData, MusicBotError>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            panic!("Failed to start bot: {:?}", error)
        }

        // `error` already carries the "Whoops…" prefix in its Display impl,
        // so render it raw — wrapping it again would nest the prefix.
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::error!("Error in command `{}`: {:?}", ctx.command().name, error);
            let embed = BotEmbed::Error(error).to_embed();
            let _ = ctx
                .send(poise::CreateReply::default().embed(embed).reply(true))
                .await;
            schedule_prefix_delete(ctx);
        }

        poise::FrameworkError::CommandCheckFailed { error, ctx, .. } => {
            if let Some(error) = error {
                let _ = ctx.reply(error.to_string()).await;
            }
            schedule_prefix_delete(ctx);
        }

        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("Error while handling error: {}", e);
            }
        }
    }
}
