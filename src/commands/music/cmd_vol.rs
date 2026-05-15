use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

/// Default cap when no `!` suffix is supplied. With a trailing `!` (e.g.
/// `!volume 200!`) the cap is raised to 500 so users can opt into overdrive.
const DEFAULT_MAX_VOLUME: f32 = 100.0;
const EXTENDED_MAX_VOLUME: f32 = 500.0;

/// Change the volume (1-100, or 1-500 with a trailing `!`).
#[poise::command(
    prefix_command,
    slash_command,
    check = "check_author_in_same_voice_channel",
    aliases("vol")
)]
pub async fn volume(
    ctx: Context<'_>,
    volume: Option<String>,
) -> Result<(), MusicBotError> {
    if let Some(raw) = volume {
        let trimmed = raw.trim();
        let (number_part, extended) = match trimmed.strip_suffix('!') {
            Some(rest) => (rest.trim_end(), true),
            None => (trimmed, false),
        };

        let parsed: f32 = number_part
            .parse()
            .map_err(|_| MusicBotError::InternalError(format!("Invalid volume value: {raw}")))?;

        let max = if extended { EXTENDED_MAX_VOLUME } else { DEFAULT_MAX_VOLUME };
        let clamped = parsed.clamp(1.0, max);

        let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
        player.set_volume(clamped).await?;

        PlayerEmbed::VolumeChanged(clamped)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    } else {
        let player: RwLockReadGuard<Player> = ctx.data().player.read().await;

        PlayerEmbed::Volume(player.volume * 100.0)
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    }

    Ok(())
}
