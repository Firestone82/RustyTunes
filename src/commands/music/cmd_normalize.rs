use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::{self, Player};
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

/// Toggle session-only cross-track loudness normalization (resets on restart).
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    aliases("norm", "loudnorm"),
)]
pub async fn normalize(ctx: Context<'_>, state: Option<String>) -> Result<(), MusicBotError> {
    let player_arc = ctx.data().player.clone();
    let mut player: RwLockWriteGuard<Player> = player_arc.write().await;

    let desired = match state.as_deref().map(str::trim).map(str::to_ascii_lowercase) {
        None => !player.normalize,
        Some(s) => match s.as_str() {
            "on" | "true" | "1" | "yes" | "y" => true,
            "off" | "false" | "0" | "no" | "n" => false,
            _ => {
                return Err(MusicBotError::InternalError(format!(
                    "Unknown normalize state `{s}`. Use `on` or `off`."
                )));
            }
        },
    };

    player.normalize = desired;

    // Re-apply (or undo) gain on the currently playing track so the toggle
    // takes effect immediately instead of waiting for the next track.
    if desired {
        // Turning on: schedule a measurement if we have a path and a handle.
        // The async helper bails if the track changes or the toggle flips
        // back off before the measurement returns.
        if let (Some(handle), Some(path), Some(track_id)) = (
            player.track_handle.clone(),
            player.current_source_path.clone(),
            player.current_track.as_ref().map(|t| t.id.clone()),
        ) {
            player::schedule_normalization_apply(player_arc.clone(), handle, path, track_id);
        }
    } else {
        // Turning off: drop the active gain back to unity so the user's
        // volume setting plays through directly.
        player.current_gain = 1.0;
        if let Some(handle) = &player.track_handle {
            let _ = handle.set_volume(player.volume);
        }
    }

    drop(player);

    PlayerEmbed::NormalizeState(desired)
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;

    Ok(())
}
