use std::sync::Arc;

use async_trait::async_trait;
use lombok::AllArgsConstructor;
use poise::serenity_prelude::GuildId;
use songbird::{Event, EventContext, EventHandler, Songbird};

#[derive(AllArgsConstructor)]
pub struct DisconnectHandler {
    guild_id: GuildId,
    handler: Arc<Songbird>,
}

#[async_trait]
impl EventHandler for DisconnectHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        println!("Disconnected from a voice channel. Cleaning up guild state.");

        let _ = self
            .handler
            .remove(self.guild_id)
            .await
            .map_err(|e| println!("Failed to remove guild songbird state from manager. Error: {:?}", e));

        // TODO: Cleanup playback state
        
        None
    }
}
