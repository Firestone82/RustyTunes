use std::sync::Arc;

use async_trait::async_trait;
use lombok::AllArgsConstructor;
use poise::serenity_prelude;
use poise::serenity_prelude::GuildId;
use serenity::all::ChannelId;
use songbird::{Event, EventContext, EventHandler, Songbird};

#[derive(AllArgsConstructor)]
pub struct InactivityHandler {
    guild_id: GuildId,
    handler: Arc<Songbird>,
    serenity_ctx: serenity_prelude::Context,
}

#[async_trait]
impl EventHandler for InactivityHandler {
    async fn act(&self, _e: &EventContext<'_>) -> Option<Event> {
        println!("Inactive handler triggered. Cleaning up guild state.");

        let channel_id: Option<ChannelId> = self.serenity_ctx.cache
            .guild(self.guild_id)
            .as_ref()
            .and_then(|guild| guild.voice_states.get(&self.serenity_ctx.cache.current_user().id))
            .and_then(|voice_state| voice_state.channel_id);
        
        let member_count: usize = self.serenity_ctx.cache
            .guild(self.guild_id)
            .as_ref()
            .map(|guild| {
                guild.voice_states
                    .values()
                    .filter(|voice_state| {
                        voice_state.channel_id == channel_id && voice_state.user_id != self.serenity_ctx.cache.current_user().id
                    })
                    .count()
            })
            .unwrap_or(0);
        
        if member_count == 0 {
            println!("Leaving empty channel an empty voice channel. Leaving channel.");
        
            let _ = self.handler
                .leave(self.guild_id)
                .await
                .map_err(|e| println!("Could not leave voice channel. Error: {:?}", e));
        }

        None
    }
}

