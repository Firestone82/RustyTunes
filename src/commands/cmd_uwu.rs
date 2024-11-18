use crate::bot::{Context, MusicBotError};
use crate::service::embed_service::SendEmbed;
use serenity::all::{Color, CreateEmbed};
use serenity::all::{Mention, Mentionable};
use uwu_rs::uwuify;

/**
* Convert provided text to UwU format
* -> This command requested by adaxiik
*/
#[poise::command(
    prefix_command, slash_command,
)]
pub async fn uwu(ctx: Context<'_>, text: Vec<String>) -> Result<(), MusicBotError> {
    let embed: CreateEmbed = CreateEmbed::new()
        .color(Color::from(0x36393F))
        .title("Converted message to UwU format:")
        .description(format!("```{}```", uwuify(&text.join(" ")).unwrap()));
    
    embed.send_context(ctx, false, None).await?;
    Ok(())
}

/**
* Convert provided text to UwU format and send it as author
* -> This command requested by adaxiik
*/
#[poise::command(
    prefix_command, slash_command,
)]
pub async fn uwu_me(ctx: Context<'_>, text: Vec<String>) -> Result<(), MusicBotError> {
    if let Context::Prefix(ctx) = ctx {
        ctx.msg.delete(&ctx.http()).await?;
    }

    let author: Mention = ctx.author().mention();
    let uwu_text: String = uwuify(&text.join(" ")).unwrap();

    ctx.say(format!("{}: {}", author, uwu_text)).await?;
    Ok(())
}
