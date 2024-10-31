use crate::player::notifier::{format_time, MessageNotify};
use serenity::all::{Color, CreateEmbed, Mentionable};

pub enum NotifyEmbed<'a> {
    InvalidTimeFormat,
    Created(&'a MessageNotify),
    Notification(&'a MessageNotify)
}

impl<'a> NotifyEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            NotifyEmbed::InvalidTimeFormat => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Invalid time format")
                    .description(r#"
                        Invalid time format. Please use the following format: `1mo 2d 3h 4m 5s`.
                        Examples:
                         - `2h  30m` - Notifies in 2 hours and 30 minutes.
                         - `1mo 15m` - Notifies in 1 month and 15 minutes.
                    "#)
            },
            NotifyEmbed::Created(notify) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Notification created")
                    .description(format!("You will be notified at `{}`", format_time(notify.notify_at)))
            },
            NotifyEmbed::Notification(notify) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Notification")
                    .description(format!("Hey {}, \nyou wanted to be notified at `{}`", notify.user_id.mention(), format_time(notify.notify_at)))
                    .field("Requested at:", format!("`{}`", format_time(notify.created_at)), true)
                    .field("Message:", create_link(notify), true)
            }
        }
    }
}

fn create_link(notify: &MessageNotify) -> String {
    format!("https://discord.com/channels/{}/{}/{}", notify.guild_id, notify.channel_id, notify.message_id)
}