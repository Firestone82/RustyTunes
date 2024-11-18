use crate::player::notifier::{format_time, MessageNotify};
use serenity::all::{Color, CreateEmbed, Mentionable};

pub enum NotifyEmbed<'a> {
    InvalidNotifyFormat,
    Created(&'a MessageNotify),
    Notification(&'a MessageNotify)
}

impl<'a> NotifyEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            NotifyEmbed::InvalidNotifyFormat => {
                let description = r#"
                Invalid notify format. Please use the following format: `notify <time> (note)`.

                Available time formats:
                 » `1mo 15s`            - Notifies in 1 month and 15 seconds.
                 » `1-1-2024`           - Notifies on 1st November 2024 at 9:00 AM.
                 » `24-12-2024_15:30`   - Notifies on 24th December 2024 at 3:30 PM.
                 » `tomorrow`           - Notifies tomorrow.
                "#;

                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Invalid notify format")
                    .description(description)
            }
            NotifyEmbed::Created(notify) => {
                let builder = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Notification created")
                    .description(format!("You will be notified at `{}`", format_time(notify.notify_at)));

                if notify.note.is_some() {
                    builder.field("Added note:", format!("```{}```", notify.note.clone().unwrap()), false)
                } else {
                    builder
                }
            },
            NotifyEmbed::Notification(notify) => {
                let builder = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Notification")
                    .description(format!("Hey {}, \nyou wanted to be notified at `{}`", notify.user_id.mention(), format_time(notify.notify_at)))
                    .field("Requested at:", format!("`{}`", format_time(notify.created_at)), true)
                    .field("Message:", create_link(notify), true);

                if notify.note.is_some() {
                    builder.field("Note:", format!("```{}```", notify.note.clone().unwrap()), false)
                } else {
                    builder
                }
            }
        }
    }
}

fn create_link(notify: &MessageNotify) -> String {
    format!("https://discord.com/channels/{}/{}/{}", notify.guild_id, notify.channel_id, notify.message_id)
}