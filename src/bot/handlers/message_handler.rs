use crate::bot::client::Context;
use crate::bot::player::playback::Track;
use serenity::all::{Color, CreateEmbed, Timestamp};

pub fn create_no_active_voice_embed() -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_ORANGE)
        .title("‼️ No active voice channel")
        .description("You need to be in a voice channel to use this command.")
        .timestamp(Timestamp::now())
}

pub fn create_playback_error_embed(error: String) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::RED)
        .title("‼️ Playback error")
        .description(format!("Error: {}", error))
}

pub fn create_now_playing_embed(track: &Track) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_GREEN)
        .title("▶️  Now playing")
        .description(format!("[{}]({})", track.metadata.title, track.metadata.track_url))
        .thumbnail(track.metadata.thumbnail_url.clone())
        .timestamp(Timestamp::now())
}

/**
* Send a reply to the user with the given embed.
*/
pub async fn send_embed(ctx: &Context<'_>, embed: CreateEmbed, reply: bool) -> Result<(), serenity::Error> {
    ctx.send(poise::CreateReply::default().embed(embed).reply(reply)).await?;
    Ok(())
}