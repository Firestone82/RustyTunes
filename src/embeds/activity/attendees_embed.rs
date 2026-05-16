use serenity::all::{Color, CreateEmbed};

/// Single attendee line — already resolved to a mention so the embed module
/// never has to touch the serenity cache.
pub struct AttendeeRow {
    pub mention: String,
}

pub struct AttendeesEmbed<'a> {
    pub rows: &'a [AttendeeRow],
}

impl<'a> AttendeesEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        let total = self.rows.len();
        let description = if self.rows.is_empty() {
            "_No attendees yet — join the voice channel or wait for `/expect`._".to_string()
        } else {
            let mut mentions: Vec<String> = self.rows.iter().map(|row| row.mention.clone()).collect();
            mentions.sort();
            mentions
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{}. {}", i + 1, m))
                .collect::<Vec<_>>()
                .join("\n")
        };

        CreateEmbed::new()
            .color(Color::DARK_TEAL)
            .title(format!("👥  Attendees ({})", total))
            .description(description)
    }
}
