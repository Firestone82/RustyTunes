use crate::bot::client::{Context, MusicBotError};
use crate::bot::handlers::channel_handler::get_user_voice_channel;
use poise::CreateReply;
use serenity::all::{ChannelId, Color, CreateEmbed};

pub async fn check_author_in_voice_channel(ctx: Context<'_>) -> Result<bool, MusicBotError> {
    let user_found: Option<bool> = ctx
        .guild()
        .map(|guild| guild.voice_states.contains_key(&ctx.author().id));

    match user_found {
        Some(false) => {
            let embed: CreateEmbed = CreateEmbed::new()
                .color(Color::RED)
                .title("No active voice channel")
                .description("You need to be in a voice channel to use this command.");

            ctx.send(CreateReply::default().embed(embed).reply(true)).await?;
            Ok(false)
        }

        Some(true) => {
            Ok(true)
        },

        None => {
            let embed: CreateEmbed = CreateEmbed::new()
                .color(Color::RED)
                .title("Could not validate if author in voice channel")
                .description("Could not validate if author in voice channel");

            ctx.send(CreateReply::default().embed(embed).reply(true)).await?;
            Ok(false)
        }
    }
}

pub async fn check_author_in_same_voice_channel(ctx: Context<'_>) -> Result<bool, MusicBotError> {
    let user_channel_id: Option<ChannelId> = get_user_voice_channel(ctx, &ctx.author().id).await;
    let bot_channel_id: Option<ChannelId> = get_user_voice_channel(ctx, &ctx.framework().bot_id).await;

    match (user_channel_id, bot_channel_id) {
        (Some(user_channel), Some(bot_channel)) => {
            if user_channel == bot_channel {
                Ok(true)
            } else {
                let embed: CreateEmbed = CreateEmbed::new()
                    .color(Color::RED)
                    .title("Not in the same voice channel")
                    .description("You need to be in the same voice channel as the bot to use this command.");

                ctx.send(CreateReply::default().embed(embed).reply(true)).await?;
                Ok(false)
            }
        }

        (Some(_), None) => {
            Ok(true)
        }

        (None, None) | (None, Some(_)) => {
            let embed: CreateEmbed = CreateEmbed::new()
                .color(Color::RED)
                .title("No active voice channel")
                .description("You need to be in a voice channel to use this command.");

            ctx.send(CreateReply::default().embed(embed).reply(true)).await?;
            Ok(false)
        }
    }
}