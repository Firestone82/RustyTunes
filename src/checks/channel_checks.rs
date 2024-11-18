use crate::bot::{Context, MusicBotError};
use crate::embeds::bot_embeds::BotEmbed;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use serenity::all::ChannelId;

pub async fn check_author_in_voice_channel(ctx: Context<'_>) -> Result<bool, MusicBotError> {
    let user_found: Option<bool> = ctx
        .guild()
        .map(|guild| guild.voice_states.contains_key(&ctx.author().id));

    match user_found {
        Some(false) => {
            BotEmbed::CurrentUserNotInVoiceChannel
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;

            Ok(false)
        }

        Some(true) => {
            Ok(true)
        },

        None => {
            println!("User not found in any voice channel");
            Ok(false)
        }
    }
}

pub async fn check_author_in_same_voice_channel(ctx: Context<'_>) -> Result<bool, MusicBotError> {
    let user_channel_id: Option<ChannelId> = channel_service::get_user_voice_channel(ctx, &ctx.author().id);
    let bot_channel_id: Option<ChannelId> = channel_service::get_user_voice_channel(ctx, &ctx.framework().bot_id);

    match (user_channel_id, bot_channel_id) {
        (Some(user_channel), Some(bot_channel)) => {
            if user_channel == bot_channel {
                Ok(true)
            } else {
                BotEmbed::CurrentUserNotInSharedChannel(&bot_channel)
                    .to_embed()
                    .send_context(ctx, true, Some(30))
                    .await?;
                
                Ok(false)
            }
        }

        (Some(_), None) => {
            Ok(true)
        }

        (None, None) | (None, Some(_)) => {
            BotEmbed::CurrentUserNotInVoiceChannel
                .to_embed()
                .send_context(ctx, true, Some(30))
                .await?;
            
            Ok(false)
        }
    }
}