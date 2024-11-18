use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::embeds::bot_embeds::BotEmbed;
use crate::service::channel_service;
use crate::service::embed_service::SendEmbed;
use serenity::all::{Channel, ChannelId, GuildId, Member, Mention, Mentionable, PartialGuild, User};

/**
* Wake up a user in the same voice channel
*/
#[poise::command(
    slash_command,
    check = "check_author_in_same_voice_channel"
)]
pub async fn wakeup(ctx: Context<'_>, target: Member, count: Option<usize>) -> Result<(), MusicBotError> {
    wakeup_target(ctx, target, count).await?;
    Ok(())
}

/**
* Wake up a user in the same voice channel
*/
#[poise::command(
    context_menu_command = "WakeUp!",
    check = "check_author_in_same_voice_channel"
)]
pub async fn wakeup_context(ctx: Context<'_>, user: User) -> Result<(), MusicBotError> {
    let guild_id: GuildId = ctx.guild().unwrap().id;
    let guild: PartialGuild = ctx.http().get_guild(guild_id).await?;

    let member: Member = guild.member(ctx.http(), user.id).await?;
    
    wakeup_target(ctx, member, None).await?;
    Ok(())
}

async fn wakeup_target(ctx: Context<'_>, target: Member, count: Option<usize>) -> Result<(), MusicBotError> {
    let afk_channel_id: ChannelId = ChannelId::new(829712736052707380);
    let afk_channel: Channel = ctx.http().get_channel(afk_channel_id).await?;

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

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

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