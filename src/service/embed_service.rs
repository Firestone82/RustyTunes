use crate::bot::{Context, MusicBotError};
use crate::player::player::{PlaybackError, Track};
use crate::service::utils_service;
use crate::sources::youtube::youtube_client::SearchError;
use serenity::all::{ChannelId, Color, CreateEmbed, CreateEmbedFooter, CreateMessage, GuildChannel, Http, Message};
use std::sync::Arc;

pub fn create_track_added_to_queue(queue: &Vec<Track>, track: &Track) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_GREEN)
        .title("🎵  Track added to queue")
        .description(format!("**[{}]({})**", track.metadata.title, track.metadata.track_url))
        .footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())))
}

pub fn create_playlist_added_to_queue(queue: &Vec<Track>, tracks: &Vec<Track>) -> CreateEmbed {
    let mut embed: CreateEmbed = CreateEmbed::new()
        .color(Color::DARK_GREEN)
        .title("🎵  Playlist added to queue")
        .description("Tracks added to queue:");

    // for track in tracks {
    //     embed = embed.field(track.metadata.title.clone(), track.metadata.track_url.clone(), false);
    // }

    embed.footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())))
}

pub fn create_playback_error_embed(error: PlaybackError) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("🚫  Playback error")
        .description(error.to_string())
}

pub fn create_search_error_embed(error: SearchError) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("🚫  Search error")
        .description(error.to_string())
}

pub fn create_track_choose_embed(tracks: &Vec<Track>) -> CreateEmbed {
    let mut embed: CreateEmbed = CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("🔍  Search results");

    let mut index = 1;
    for track in tracks.clone() {
        embed = embed.field(format!(":number_{}:  {}", index, track.metadata.title), track.metadata.track_url, false);
        index += 1;
    }

    embed
}

pub fn create_skip_embed(amount: usize) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("⏭️  Skipped")
        .description(format!("Skipped {} track(s).", amount))
}

pub fn create_track_choose_expired_embed() -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("🚫  Search expired")
        .description("The search has expired. Please try again.")
}

pub fn create_user_not_in_voice_embed() -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("🚫  User not in voice channel")
        .description("You must be in a voice channel to use this command.")
}

pub fn create_user_not_in_shared_voice_channel_embed(bot_channel: ChannelId) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("🚫  User not in shared voice channel")
        .field("Bot channel:", format!("<#{}> - click to join", bot_channel), true)
        .description("You must be in the same voice channel as the bot to use this command.")
}

pub fn create_error_embed(error: MusicBotError) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("🚫  Error")
        .description(error.to_string())
}

pub fn create_now_playing_embed(track: &Track) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("🎵  Now playing")
        .description(format!("**[{}]({})**", track.metadata.title, track.metadata.track_url))
}

pub fn create_no_song_playing_embed() -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("🚫  No song playing")
        .description("No song is currently playing.")
}

pub fn create_shuffle_song_embed() -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("🔀  Shuffle")
        .description("Queue has been shuffled.")
}

pub fn create_playback_stopped_embed() -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_RED)
        .title("⏹️  Playback stopped")
        .description("The playback has been stopped.")
}

pub fn create_volume_change_embed(volume: f32) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("🔊  Volume changed")
        .description(format!("Volume set to {}%.", volume))
}

pub fn create_volume_embed(volume: f32) -> CreateEmbed {
    CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("🔊  Volume")
        .description(format!("Volume is set to {}%.", volume))
}

pub fn create_queue_embed(queue: &Vec<Track>, page: usize) -> CreateEmbed {
    let mut embed: CreateEmbed = CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("📜  Queue")
        .description("Upcoming tracks:")
        .footer(CreateEmbedFooter::new(format!("Queue length: {}", queue.len())));

    let page: usize = page.max(1);
    let mut start: usize = (page - 1) * 10;

    if start >= queue.len() {
        start = queue.len().saturating_sub(1);
    }

    let queue_slice: Vec<&Track> = queue.iter().skip(start).take(10).collect::<Vec<&Track>>();

    for (index, track) in queue_slice.iter().enumerate() {
        embed = embed.field(
            format!("{}  {}", utils_service::number_to_emoji(index + start + 1), track.metadata.title),
            &track.metadata.track_url,
            false,
        );
    }

    embed
}

/*
 * Send embeds
 */

pub async fn send_channel_embed(http: Arc<Http>, channel: &GuildChannel, embed: CreateEmbed, delete_after: Option<u64>) -> Result<Message, MusicBotError> {
    let created_message = CreateMessage::default()
        .embed(embed);

    let message = channel.send_message(http.clone(), created_message).await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    process_message(http, &message, delete_after).await;

    Ok(message)
}

pub async fn send_context_embed(ctx: Context<'_>, embed: CreateEmbed, reply: bool, delete_after: Option<u64>) -> Result<Message, MusicBotError> {
    let created_reply = poise::CreateReply::default()
        .embed(embed)
        .reply(reply);

    let reply_handle = ctx.send(created_reply).await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    let message = reply_handle.into_message().await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    let http = ctx.serenity_context().http.clone();
    process_message(http, &message, delete_after).await;

    Ok(message)
}

async fn process_message(http: Arc<Http>, message: &Message, delete_after: Option<u64>) {
    let channel_id = message.channel_id.clone();
    let message_id = message.id.clone();

    if let Some(seconds) = delete_after {
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
            let _ = http.delete_message(channel_id,message_id, Some("Cleaning up last message")).await;
        });
    }
}