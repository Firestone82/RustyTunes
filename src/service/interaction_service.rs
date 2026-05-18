//! Safe handling of Discord component (button) interactions.
//!
//! Discord requires every interaction to be acknowledged within ~3 seconds.
//! When several users mash the same button at once, a single-task collector
//! loop processes them serially — the second click can easily blow past the
//! window while the first is still doing HTTP work, at which point Discord
//! gives the second user "This interaction failed".
//!
//! The fix is to acknowledge each interaction **before** any non-trivial
//! work: call [`ack`] as the first step of every handler, then deliver the
//! actual result through [`edit_message`] (for in-place updates) or
//! [`reply_ephemeral`] (for private replies). This keeps the ack within the
//! 3 s budget regardless of how long subsequent processing takes.

use serenity::all::{CacheHttp, ComponentInteraction, CreateInteractionResponseFollowup, EditInteractionResponse, Message, Result as SerenityResult};

/// Acknowledge the interaction with `DeferredUpdateMessage`. Must be the
/// first call in any component handler — everything after this can take its
/// time without tripping the 3-second timeout. Subsequent [`edit_message`]
/// or [`reply_ephemeral`] calls resolve the deferred response.
pub async fn ack(
    ic: &ComponentInteraction,
    http: impl CacheHttp,
) -> SerenityResult<()> {
    ic.defer(http).await
}

/// Replace the message the component lives on, after [`ack`]. Drop-in
/// replacement for the old `CreateInteractionResponse::UpdateMessage(_)`
/// pattern that used to be passed straight to `create_response`.
pub async fn edit_message(
    ic: &ComponentInteraction,
    http: impl CacheHttp,
    builder: EditInteractionResponse,
) -> SerenityResult<Message> {
    ic.edit_response(http, builder).await
}

/// Send a reply visible only to the clicker after [`ack`]. Used for gating
/// messages like "Only the person who started this can cancel."
pub async fn reply_ephemeral(
    ic: &ComponentInteraction,
    http: impl CacheHttp,
    content: impl Into<String>,
) -> SerenityResult<Message> {
    ic.create_followup(
        http,
        CreateInteractionResponseFollowup::new()
            .content(content.into())
            .ephemeral(true),
    )
    .await
}
