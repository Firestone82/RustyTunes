use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::player_embed::PlayerEmbed;
use crate::player::player::Player;
use crate::service::embed_service::SendEmbed;
use tokio::sync::RwLockWriteGuard;

/// Which source(s) the user's `!normalize` invocation should affect.
enum Target {
    YouTube,
    Spotify,
    Local,
    All,
}

impl Target {
    fn parse(input: &str) -> Option<Target> {
        match input.trim().to_ascii_lowercase().as_str() {
            "youtube" | "yt" | "y" => Some(Target::YouTube),
            "spotify" | "sp" | "s" => Some(Target::Spotify),
            "local" | "file" | "files" | "l" => Some(Target::Local),
            "all" | "*" => Some(Target::All),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Target::YouTube => "YouTube",
            Target::Spotify => "Spotify",
            Target::Local => "Local",
            Target::All => "All sources",
        }
    }
}

fn parse_state(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" | "y" => Some(true),
        "off" | "false" | "0" | "no" | "n" => Some(false),
        _ => None,
    }
}

/// Toggle session-only loudness normalization, per source (resets on restart).
#[poise::command(
    prefix_command, slash_command,
    check = "check_author_in_same_voice_channel",
    aliases("norm", "loudnorm"),
)]
pub async fn normalize(
    ctx: Context<'_>,
    target: Option<String>,
    state: Option<String>,
) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;

    let changed_label: Option<&str> = match target.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => {
            let target = Target::parse(raw).ok_or_else(|| {
                MusicBotError::InternalError(format!(
                    "Unknown target `{raw}`. Use `youtube`, `spotify`, `local`, or `all`."
                ))
            })?;

            // Without an explicit state: toggle. With one: set absolute.
            let resolve = |current: bool| -> Result<bool, MusicBotError> {
                match state.as_deref() {
                    None => Ok(!current),
                    Some(s) => parse_state(s).ok_or_else(|| {
                        MusicBotError::InternalError(format!(
                            "Unknown state `{s}`. Use `on` or `off`."
                        ))
                    }),
                }
            };

            match target {
                Target::YouTube => {
                    player.normalize_youtube = resolve(player.normalize_youtube)?;
                }
                Target::Spotify => {
                    player.normalize_spotify = resolve(player.normalize_spotify)?;
                }
                Target::Local => {
                    player.normalize_local = resolve(player.normalize_local)?;
                }
                Target::All => {
                    // For "all" without state, toggle relative to whether
                    // anything is currently on — flipping everything to
                    // either fully-on or fully-off rather than per-source.
                    let new_state = match state.as_deref() {
                        None => !(player.normalize_youtube
                            || player.normalize_spotify
                            || player.normalize_local),
                        Some(s) => parse_state(s).ok_or_else(|| {
                            MusicBotError::InternalError(format!(
                                "Unknown state `{s}`. Use `on` or `off`."
                            ))
                        })?,
                    };
                    player.normalize_youtube = new_state;
                    player.normalize_spotify = new_state;
                    player.normalize_local = new_state;
                }
            }
            Some(target.label())
        }
    };

    let snapshot = (
        player.normalize_youtube,
        player.normalize_spotify,
        player.normalize_local,
    );
    drop(player);

    PlayerEmbed::NormalizeState {
        youtube: snapshot.0,
        spotify: snapshot.1,
        local: snapshot.2,
        changed: changed_label,
    }
        .to_embed()
        .send_context(ctx, true, Some(30))
        .await?;

    Ok(())
}
