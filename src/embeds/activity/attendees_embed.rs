use serenity::all::{Color, CreateEmbed};

/// Single attendee line — already resolved to a mention so the embed module
/// never has to touch the serenity cache.
pub struct AttendeeRow {
    pub mention: String,
    pub in_voice: bool,
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
            let mut lines: Vec<String> = self
                .rows
                .iter()
                .map(|row| {
                    let icon = if row.in_voice { "🔊" } else { "⏳" };
                    let label = if row.in_voice { "in voice" } else { "expected" };
                    format!("{} {} — {}", icon, row.mention, label)
                })
                .collect();
            lines.sort();
            lines.join("\n")
        };

        CreateEmbed::new()
            .color(Color::DARK_TEAL)
            .title(format!("👥  Attendees ({})", total))
            .description(description)
    }
}
