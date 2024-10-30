use serenity::all::{ChannelId, Color, CreateEmbed};
use crate::bot::MusicBotError;

pub enum BotEmbed<'a> {
    UserNotInVoiceChannel,
    UserNotInSharedChannel(&'a ChannelId),
    Error(MusicBotError)
}

impl<'a> BotEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            BotEmbed::UserNotInVoiceChannel => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  User not in voice channel")
                    .description("You need to be in a voice channel to use this command.")
            },
            BotEmbed::UserNotInSharedChannel(channel_id) => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  User not in same voice channel")
                    .field("Bot channel:", format!("<#{}> - click to join", channel_id), true)
                    .description("You need to be in the same voice channel as the bot to use this command.")
            },
            BotEmbed::Error(error) => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Error")
                    .description(error.to_string())
            }
        }
    }
}