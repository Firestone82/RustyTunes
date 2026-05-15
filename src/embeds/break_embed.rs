use crate::service::utils_service::{format_wall_clock, humanize_duration};
use serenity::all::{
    ButtonStyle, Color, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter,
};
use std::time::Duration;
use time::OffsetDateTime;

pub const BTN_BREAK_CANCEL: &str = "break_cancel";
pub const BTN_BREAK_SKIP: &str = "break_skip";

pub enum BreakEmbed<'a> {
    TooLong {
        max: Duration,
    },
    InvalidDuration,
    AlreadyRunning,
    InvalidExtension,
    NoActiveBreak,
    ExceedsCap {
        new_total: Duration,
        cap: Duration,
    },
    Extended {
        author_mention: &'a str,
        extra: Duration,
        total: Duration,
        ends_at: OffsetDateTime,
    },
    Progress {
        author_mention: &'a str,
        clock_time_label: Option<&'a str>,
        original_duration: Duration,
        remaining: Duration,
        extension: Duration,
        total: Duration,
        footer: Option<&'a str>,
    },
}

impl<'a> BreakEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            BreakEmbed::TooLong { max } => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Break too long")
                .description(format!(
                    "Maximum break length is {}.",
                    humanize_duration(*max)
                )),
            BreakEmbed::InvalidDuration => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Invalid break duration")
                .description(
                    "Use a relative duration like `5m`, `1h 30s`, or `90s`, \
                     or a clock time like `10:00` or `14:30`.",
                ),
            BreakEmbed::AlreadyRunning => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Break already running")
                .description(
                    "There's already an active break in this guild — extend it with \
                     `/break extend <time>` instead.",
                ),
            BreakEmbed::InvalidExtension => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Invalid extension")
                .description("Use a relative duration like `5m`, `1h 30s`, or `90s`."),
            BreakEmbed::NoActiveBreak => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  No active break")
                .description(
                    "There's no break running right now. Start one with `/break start <time>`.",
                ),
            BreakEmbed::ExceedsCap { new_total, cap } => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Extension would exceed cap")
                .description(format!(
                    "Total break length would be `{}`, over the {} cap.",
                    humanize_duration(*new_total),
                    humanize_duration(*cap),
                )),
            BreakEmbed::Extended {
                author_mention,
                extra,
                total,
                ends_at,
            } => CreateEmbed::new()
                .color(Color::DARK_GREEN)
                .title("⏱️  Break extended")
                .description(format!(
                    "{} extended the break by **{}**.\n\n\
                     New total: **{}**\n\
                     Ends at: `{}`",
                    author_mention,
                    humanize_duration(*extra),
                    humanize_duration(*total),
                    format_wall_clock(*ends_at),
                )),
            BreakEmbed::Progress {
                author_mention,
                clock_time_label,
                original_duration,
                remaining,
                extension,
                total,
                footer,
            } => {
                let color = if footer.is_some() {
                    Color::DARK_GREEN
                } else {
                    Color::DARK_GOLD
                };

                let opening = match clock_time_label {
                    Some(label) => format!("{} started a break until **{}**.", author_mention, label),
                    None => format!(
                        "{} started a break of **{}**.",
                        author_mention,
                        humanize_duration(*original_duration)
                    ),
                };

                let mut description =
                    format!("{}\n\nTime remaining: **{}**", opening, humanize_duration(*remaining));

                if *extension > Duration::ZERO {
                    description.push_str(&format!(
                        "\nExtended by: **{}** (total **{}**)",
                        humanize_duration(*extension),
                        humanize_duration(*total),
                    ));
                }

                description.push_str(
                    "\n\nWhen the timer ends, everyone still in voice will be gathered automatically
                    \n— late arrivals will be tracked.",
                );

                let mut builder = CreateEmbed::new()
                    .color(color)
                    .title("⏸️  Break in progress")
                    .description(description);

                if let Some(text) = footer {
                    builder = builder.footer(CreateEmbedFooter::new(*text));
                }

                builder
            }
        }
    }
}

pub fn break_buttons(disabled: bool) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(BTN_BREAK_SKIP)
            .label("Skip to gathering")
            .style(ButtonStyle::Primary)
            .disabled(disabled),
        CreateButton::new(BTN_BREAK_CANCEL)
            .label("Cancel")
            .style(ButtonStyle::Danger)
            .disabled(disabled),
    ])]
}
