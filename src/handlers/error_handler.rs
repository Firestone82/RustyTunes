use crate::bot::{MusicBotData, MusicBotError};
use crate::embeds::bot::bot_embeds::BotEmbed;
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::service::embed_service::SendEmbed;
use crate::utils::ytdlp_utils::summarize_ytdlp_error;
use async_trait::async_trait;
use poise::serenity_prelude;
use serenity::all::GuildChannel;
use songbird::tracks::PlayMode;
use songbird::{Event, EventContext, EventHandler};

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

/// Per-track Songbird handler for `TrackEvent::Error`. Captures the track's
/// title and announcement channel at registration time so it can post a
/// "Track failed" embed when songbird reports the input died. The End event
/// still fires for errored tracks (songbird emits both), so queue advance is
/// handled by `QueueHandler` — this handler only owns the user-facing notice.
#[derive(Clone)]
pub struct TrackErrorHandler {
    serenity_ctx: serenity_prelude::Context,
    guild_channel: GuildChannel,
    track_title: String,
}

impl TrackErrorHandler {
    pub fn new(
        serenity_ctx: serenity_prelude::Context,
        guild_channel: GuildChannel,
        track_title: String,
    ) -> Self {
        Self {
            serenity_ctx,
            guild_channel,
            track_title,
        }
    }
}

#[async_trait]
impl EventHandler for TrackErrorHandler {
    async fn act(
        &self,
        ctx: &EventContext<'_>,
    ) -> Option<Event> {
        let raw_error = if let EventContext::Track(states) = ctx {
            states.first().and_then(|(state, _)| match &state.playing {
                PlayMode::Errored(err) => Some(err.to_string()),
                _ => None,
            })
        } else {
            None
        };

        let reason = raw_error
            .as_deref()
            .map(summarize_ytdlp_error)
            .unwrap_or_else(|| "playback error".to_string());

        tracing::error!(
            "Track failed: '{}' — {} (raw: {:?})",
            self.track_title,
            reason,
            raw_error
        );

        let _ = PlayerEmbed::TrackFailed {
            title: self.track_title.clone(),
            reason,
        }
        .to_embed()
        .send_channel(
            self.serenity_ctx.http.clone(),
            &self.guild_channel,
            Some(60),
            None,
        )
        .await;

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
