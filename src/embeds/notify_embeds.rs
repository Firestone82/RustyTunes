use crate::player::notifier::{format_time, MessageNotify};
use serenity::all::{Color, CreateEmbed, Mentionable};

pub enum NotifyEmbed<'a> {
    InvalidNotifyFormat,
    Created(&'a MessageNotify),
    Notification(&'a MessageNotify),
    List(&'a [MessageNotify]),
    Removed(&'a MessageNotify),
    NotFound,
    RemindedFor {
        targets: &'a str,
        notify: &'a MessageNotify,
        note: Option<&'a str>,
    },
}

impl<'a> NotifyEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            NotifyEmbed::InvalidNotifyFormat => {
                let description = r#"
                Invalid notify format. Use: `notify <time> [note]`.

                Available time formats:
                 » `10s`                - Notifies in 10 seconds.
                 » `7d`                 - Notifies in 7 days.
                 » `1mo 15s`            - Combined relative offsets.
                 » `1-1-2024`           - Notifies on 1st January 2024 at 9:00.
                 » `24-12-2024_15:30`   - Notifies on 24th December 2024 at 15:30.
                 » `tomorrow` / `week`  - Convenience literals.
                "#;

                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Invalid notify format")
                    .description(description)
            }
            NotifyEmbed::Created(notify) => {
                let mut builder = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Notification created")
                    .description(format!(
                        "ID `#{}` — you will be notified at `{}`",
                        notify.id,
                        format_time(notify.notify_at)
                    ));

                if let Some(note) = notify.display_note() {
                    builder = builder.field("Note:", format!("```{}```", note), false);
                }

                builder
            }
            NotifyEmbed::Notification(notify) => {
                let targets = notify.targets();
                let description = if targets.is_empty() {
                    format!(
                        "Hey {}, you wanted to be notified at `{}`",
                        notify.user_id.mention(),
                        format_time(notify.notify_at)
                    )
                } else {
                    let mentions = targets
                        .iter()
                        .map(|u| u.mention().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "Hey {}, you have a reminder at `{}`",
                        mentions,
                        format_time(notify.notify_at)
                    )
                };

                let mut builder = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Notification")
                    .description(description)
                    .field("Requested at:", format!("`{}`", format_time(notify.created_at)), true);

                if !targets.is_empty() {
                    builder = builder.field("From:", notify.user_id.mention().to_string(), true);
                }

                if let Some(link) = create_link(notify) {
                    builder = builder.field("Message:", link, true);
                }

                if let Some(note) = notify.display_note() {
                    builder = builder.field("Note:", format!("```{}```", note), false);
                }

                builder
            }
            NotifyEmbed::List(items) => {
                if items.is_empty() {
                    return CreateEmbed::new()
                        .color(Color::DARK_BLUE)
                        .title("🔔  Notifications")
                        .description("You have no pending notifications.");
                }

                let mut description = String::new();
                for n in items.iter() {
                    let display = n.display_note();
                    let note_preview = match display.as_deref() {
                        Some(s) if !s.is_empty() => {
                            let cut: String = s.chars().take(60).collect();
                            format!(" — {}", cut)
                        }
                        _ => String::new(),
                    };
                    description.push_str(&format!(
                        "`#{:>3}` `{}`{}\n",
                        n.id,
                        format_time(n.notify_at),
                        note_preview
                    ));
                }

                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Your notifications")
                    .description(description)
            }
            NotifyEmbed::Removed(notify) => {
                CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🗑️  Notification removed")
                    .description(format!(
                        "Removed notification `#{}` scheduled for `{}`.",
                        notify.id,
                        format_time(notify.notify_at)
                    ))
            }
            NotifyEmbed::NotFound => {
                CreateEmbed::new()
                    .color(Color::DARK_RED)
                    .title("🚫  Notification not found")
                    .description("No notification with that id belongs to you in this guild.")
            }
            NotifyEmbed::RemindedFor { targets, notify, note } => {
                let mut builder = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("🔔  Reminder set")
                    .description(format!(
                        "Will remind {} at `{}`.",
                        targets,
                        format_time(notify.notify_at)
                    ));

                if let Some(text) = note.filter(|s| !s.is_empty()) {
                    builder = builder.field("Note:", format!("```{}```", text), false);
                }

                builder
            }
        }
    }
}

fn create_link(notify: &MessageNotify) -> Option<String> {
    let message_id = notify.message_id?;
    Some(format!(
        "https://discord.com/channels/{}/{}/{}",
        notify.guild_id, notify.channel_id, message_id
    ))
}
