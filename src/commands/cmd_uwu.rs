use crate::bot::{Context, MusicBotError};
use crate::service::embed_service::SendEmbed;
use serenity::all::{Color, CreateEmbed};
use serenity::all::{Mention, Mentionable};
use uwuifier::uwuify_str_sse;

/**
 * This command requested by adaxiik
 */
#[poise::command(
    prefix_command, slash_command,
)]
pub async fn uwu(ctx: Context<'_>, text: Vec<String>) -> Result<(), MusicBotError> {
    let embed: CreateEmbed = CreateEmbed::new()
        .color(Color::from(0x36393F))
        .title("Converted message to UwU format:")
        .description(format!("```{}```", uwuify_str_sse(&text.join(" "))));
    
    embed.send_context(ctx, false, None).await?;
    Ok(())
}

#[poise::command(
    prefix_command, slash_command,
)]
pub async fn uwu_me(ctx: Context<'_>, text: Vec<String>) -> Result<(), MusicBotError> {
    if let Context::Prefix(ctx) = ctx {
        ctx.msg.delete(&ctx.http()).await?;
    }

    let author: Mention = ctx.author().mention();
    let uwu_text: String = uwuify_str_sse(&text.join(" "));

    ctx.say(format!("{}: {}", author, uwu_text)).await?;
    Ok(())
}
