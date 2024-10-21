use std::sync::Arc;

use async_trait::async_trait;
use lombok::AllArgsConstructor;
use poise::serenity_prelude::{Cache, GuildId};
use songbird::{Event, EventContext, EventHandler, Songbird};

#[derive(AllArgsConstructor)]
pub struct InactivityHandler {
    guild_id: GuildId,
    handler: Arc<Songbird>,
    cache: Arc<Cache>,
}

#[async_trait]
impl EventHandler for InactivityHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        println!("Inactive handler triggered. Cleaning up guild state.");

        let channel_id = self
            .cache
            .guild(self.guild_id)
            .as_ref()
            .and_then(|guild| guild.voice_states.get(&self.cache.current_user().id))
            .and_then(|voice_state| voice_state.channel_id);

        let member_count = self
            .cache
            .guild(self.guild_id)
            .as_ref()
            .map(|guild| {
                guild
                    .voice_states
                    .values()
                    .filter(|voice_state| {
                        voice_state.channel_id == channel_id && voice_state.member.as_ref().is_some_and(|m| !m.user.bot)
                    })
                    .count()
            })
            .unwrap_or(0);

        if member_count == 0 {
            println!("Leaving empty channel an empty voice channel. Leaving channel.");
            
            let _ = self
                .handler
                .leave(self.guild_id)
                .await
                .map_err(|e| println!("Could not leave voice channel. Error: {:?}", e));
        }
        
        None
    }
}

