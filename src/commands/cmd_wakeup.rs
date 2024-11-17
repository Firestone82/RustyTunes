use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::bot_embeds::BotEmbed;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use serenity::all::{ChannelId, Member, Mention, Mentionable};

#[poise::command(
    slash_command,
    check = "check_author_in_same_voice_channel"
)]
pub async fn wakeup(ctx: Context<'_>, target: Member, count: Option<usize>) -> Result<(), MusicBotError> {
    let afk_channel_id: ChannelId = ChannelId::new(829712736052707380);
    let afk_channel = ctx.http().get_channel(afk_channel_id).await?;

    let current_channel: Option<ChannelId> = channel_service::get_user_voice_channel(ctx, &ctx.author().id);
    let count: usize = count.unwrap_or(2).min(5).max(1);

    if let Some(user_channel) = current_channel {
        let author_m: Mention = ctx.author().mention();
        let target_m: Mention = target.mention();

        ctx.say(format!("{}: Hey {}, wake up!", author_m, target_m)).await?;
        
        for _ in 0..count {
            if let Err(_) = target.move_to_voice_channel(ctx.http(), afk_channel.clone()).await {
                BotEmbed::Error(MusicBotError::InternalError("Failed to move user to AFK channel".to_string()))
                    .to_embed()
                    .send_context(ctx, true, Some(30))
                    .await?;
                return Ok(());
            }
            
            if let Err(_) = target.move_to_voice_channel(ctx.http(), user_channel).await {
                BotEmbed::Error(MusicBotError::InternalError("Failed to move user back to original channel".to_string()))
                    .to_embed()
                    .send_context(ctx, true, Some(30))
                    .await?;
                return Ok(());
            }
        }
    } else {
        BotEmbed::CurrentUserNotInVoiceChannel
            .to_embed()
            .send_context(ctx, true, Some(30))
            .await?;
    }
    
    Ok(())
}