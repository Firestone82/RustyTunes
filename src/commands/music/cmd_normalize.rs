use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

/// Toggle session-only loudness normalization (resets on restart).
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    aliases("normalizer", "norm"),
)]
pub async fn normalize(ctx: Context<'_>, state: Option<String>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    let desired = match state.as_deref().map(str::trim).map(str::to_ascii_lowercase) {
        None => !player.normalize,
        Some(s) => match s.as_str() {
            "on" | "true" | "1" | "yes" | "y" => true,
            "off" | "false" | "0" | "no" | "n" => false,
            _ => {
                return Err(MusicBotError::InternalError(format!(
                    "Unknown normalizer state `{s}`. Use `on` or `off`."
                )));
            }
        },
    };

    player.normalize = desired;
    drop(player);

    PlayerEmbed::NormalizeState(desired)
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;

    Ok(())
}
