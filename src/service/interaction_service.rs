use serenity::all::{ComponentInteraction, ComponentInteractionCollector, MessageId};
use serenity::futures::StreamExt;
use serenity::prelude::Context as SerenityContext;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Buffer button clicks for one message, defer each one immediately on
/// arrival, and hand the deferred interactions back for serial processing.
///
/// Why this exists: Discord expires a component interaction token 3 seconds
/// after the click. If the bot processes clicks serially and a single
/// handler takes longer than that (e.g. lock contention, yt-dlp probe), any
/// click that landed in the buffer during the slow handler has already
/// expired by the time we pop it. Deferring each click in the forwarder loop
/// — instead of waiting our turn in the main loop — keeps the ack within the
/// 3-second window regardless of how busy the consumer is. Deferring is done
/// sequentially (not spawned) so the original click order is preserved; a
/// single defer round-trip is ~100–300 ms, so even a burst of several rapid
/// clicks stays comfortably under the 3-second limit.
///
/// Consumers receive interactions that have already been acked via
/// `defer()` (a silent `DeferredUpdateMessage`), so subsequent updates must
/// go through `message.edit(...)`, `create_followup(...)`, or
/// `edit_response(...)` — not `create_response(...)`.
pub struct DeferredInteractionStream {
    rx: mpsc::UnboundedReceiver<ComponentInteraction>,
    forwarder: JoinHandle<()>,
}

impl DeferredInteractionStream {
    pub fn new(
        serenity_ctx: &SerenityContext,
        message_id: MessageId,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let http = serenity_ctx.http.clone();
        let collector = ComponentInteractionCollector::new(serenity_ctx.clone())
            .message_id(message_id)
            .stream();

        let forwarder = tokio::spawn(async move {
            tokio::pin!(collector);
            while let Some(ic) = collector.next().await {
                // Defer inline (not spawned) so click order is preserved.
                // Deferring is fast (~100-300 ms) and sequential deferral of
                // a burst still finishes well within Discord's 3-second ack
                // window.
                if let Err(error) = ic.defer(&http).await {
                    tracing::debug!("Failed to defer component interaction: {:?}", error);
                    continue;
                }
                let _ = tx.send(ic);
            }
        });

        Self { rx, forwarder }
    }

    /// Wait for the next deferred interaction, returning `None` when the
    /// collector ends.
    pub async fn next(&mut self) -> Option<ComponentInteraction> {
        self.rx.recv().await
    }

    /// Wait up to `timeout` for the next deferred interaction. Returns
    /// `None` on timeout or when the collector ends.
    pub async fn next_within(
        &mut self,
        timeout: Duration,
    ) -> Option<ComponentInteraction> {
        tokio::time::timeout(timeout, self.rx.recv())
            .await
            .ok()
            .flatten()
    }
}

impl Drop for DeferredInteractionStream {
    fn drop(&mut self) {
        self.forwarder.abort();
    }
}
