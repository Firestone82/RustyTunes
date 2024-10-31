use std::sync::Arc;

use crate::player::player::Player;
use async_trait::async_trait;
use lombok::AllArgsConstructor;
use poise::serenity_prelude::GuildId;
use songbird::{Event, EventContext, EventHandler, Songbird};
use tokio::sync::{RwLock, RwLockWriteGuard};

#[derive(AllArgsConstructor)]
pub struct DisconnectHandler {
    guild_id: GuildId,
    handler: Arc<Songbird>,
    player: Arc<RwLock<Player>>
}

#[async_trait]
impl EventHandler for DisconnectHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        println!("Disconnected from a voice channel. Cleaning up guild state.");

        // TODO: Handle it efficiently. Currently its a bit trashy. Bot leaves the channel if he gets moved to another channel.
        
        let _ = self.handler
            .remove(self.guild_id)
            .await
            .map_err(|e| println!("Failed to remove guild songbird state from manager. Error: {:?}", e));

        let mut player: RwLockWriteGuard<Player> = self.player.write().await;
        let _ = player.stop_playback().await;

        None
    }
}
