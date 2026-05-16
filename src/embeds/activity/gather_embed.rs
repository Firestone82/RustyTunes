use crate::utils::string_utils::MAX_NAME_LEN;
use crate::utils::time_utils::{format_mmss, format_wall_clock, humanize_duration};
use serenity::all::{ButtonStyle, Color, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

pub const BTN_HERE: &str = "gather_im_here";
pub const BTN_CANCEL: &str = "gather_cancel";
pub const BTN_FORCE_START: &str = "gather_force_start";
pub const BTN_TOGGLE_SILENT: &str = "gather_toggle_silent";

pub const GRACE_PERIOD: Duration = Duration::from_secs(60);

/// One row in the gathering check-in table — already resolved to a display
/// name so the embed module never has to touch the serenity cache.
pub struct CheckInRow {
    pub display_name: String,
    /// `None` = not arrived, `Some(ZERO)` = on time, `Some(d>0)` = late by `d`.
    pub arrived: Option<Duration>,
}

pub enum GatherEmbed<'a> {
    InvalidPregatherTime,
    InvalidExtension,
    AlreadyRunning,
    NoActiveGathering,
    NotInPregather,
    ExceedsCap {
        new_total: Duration,
        cap: Duration,
    },
    UsersExpected {
        names: &'a str,
    },
    UsersForgotten {
        names: &'a str,
    },
    Extended {
        author_mention: &'a str,
        extra: Duration,
        total: Duration,
        ends_at: OffsetDateTime,
    },
    Pregather {
        ends_at: Instant,
        ends_at_wall: OffsetDateTime,
        author_mention: &'a str,
        schedule_label: &'a str,
        extension: Duration,
        original_duration: Duration,
        expected_mentions: Option<&'a str>,
        footer: Option<&'a str>,
    },
    CheckIn {
        rows: &'a [CheckInRow],
        started_at: Instant,
        grace_ends_at: Instant,
        silent: bool,
        footer: Option<&'a str>,
    },
}

impl<'a> GatherEmbed<'a> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            GatherEmbed::InvalidPregatherTime => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Invalid time")
                .description(
                    "Use a relative duration like `10m` or `1h 30m`, \
                     or a clock time like `10:00` or `14:30`.",
                ),
            GatherEmbed::InvalidExtension => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Invalid extension")
                .description("Use a relative duration like `5m`, `1h 30s`, or `90s`."),
            GatherEmbed::AlreadyRunning => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Gathering already running")
                .description("There's already an active gathering in this guild."),
            GatherEmbed::NoActiveGathering => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  No active gathering")
                .description("There's no gathering running right now. Start one with `/gather start`."),
            GatherEmbed::NotInPregather => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Gathering already started")
                .description("The pre-gather countdown is already over — there's nothing left to extend."),
            GatherEmbed::ExceedsCap { new_total, cap } => CreateEmbed::new()
                .color(Color::DARK_RED)
                .title("🚫  Extension would exceed cap")
                .description(format!(
                    "Total pre-gather length would be `{}`, over the {} cap.",
                    humanize_duration(*new_total),
                    humanize_duration(*cap),
                )),
            GatherEmbed::UsersExpected { names } => CreateEmbed::new()
                .color(Color::DARK_GREEN)
                .title("✅  Users expected")
                .description(format!("{} added to the gathering.", names)),
            GatherEmbed::UsersForgotten { names } => CreateEmbed::new()
                .color(Color::DARK_GREEN)
                .title("✅  Users forgotten")
                .description(format!("{} removed from the gathering.", names)),
            GatherEmbed::Extended {
                author_mention,
                extra,
                total,
                ends_at,
            } => CreateEmbed::new()
                .color(Color::DARK_GREEN)
                .title("⏱️  Gathering extended")
                .description(format!(
                    "{} extended the pre-gather countdown by **{}**.\n\n\
                     New total: **{}**\n\
                     Starts at: `{}`",
                    author_mention,
                    humanize_duration(*extra),
                    humanize_duration(*total),
                    format_wall_clock(*ends_at),
                )),
            GatherEmbed::Pregather {
                ends_at,
                ends_at_wall,
                author_mention,
                schedule_label,
                extension,
                original_duration,
                expected_mentions,
                footer,
            } => {
                let remaining = ends_at.saturating_duration_since(Instant::now());
                let mut description = format!(
                    "{} scheduled gathering {}.\n\n\
                     Time remaining: **{}**\n\
                     Starts at: `{}`",
                    author_mention,
                    schedule_label,
                    humanize_duration(remaining),
                    format_wall_clock(*ends_at_wall),
                );
                if *extension > Duration::ZERO {
                    description.push_str(&format!(
                        "\nExtended by: **{}** (total **{}**)",
                        humanize_duration(*extension),
                        humanize_duration(*original_duration + *extension),
                    ));
                }
                if let Some(mentions) = expected_mentions {
                    description.push_str(&format!("\nExpecting: {}", mentions));
                }
                description.push_str(
                    "\n\nWhen the timer ends, everyone still in voice will be gathered automatically \
                     — late arrivals will be tracked.",
                );

                let mut builder = CreateEmbed::new()
                    .color(Color::DARK_BLUE)
                    .title("📣  Voice Channel Gathering")
                    .description(description);
                if let Some(text) = footer {
                    builder = builder.footer(CreateEmbedFooter::new(*text));
                }
                builder
            }
            GatherEmbed::CheckIn {
                rows,
                started_at,
                grace_ends_at,
                silent,
                footer,
            } => build_check_in_embed(rows, *started_at, *grace_ends_at, *silent, *footer),
        }
    }
}

fn build_check_in_embed(
    rows: &[CheckInRow],
    started_at: Instant,
    grace_ends_at: Instant,
    silent: bool,
    footer: Option<&str>,
) -> CreateEmbed {
    let now = Instant::now();
    let in_grace = now < grace_ends_at;
    let grace_remaining = grace_ends_at.saturating_duration_since(now);

    // Sort: arrived first by lateness ascending; missing alphabetically.
    let mut sorted: Vec<&CheckInRow> = rows.iter().collect();
    sorted.sort_by(|a, b| match (a.arrived, b.arrived) {
        (Some(da), Some(db)) => da
            .cmp(&db)
            .then_with(|| a.display_name.cmp(&b.display_name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.display_name.cmp(&b.display_name),
    });

    let cells: Vec<(String, String)> = sorted
        .iter()
        .map(|row| {
            let status = match row.arrived {
                Some(d) if d.is_zero() => "ON TIME".to_string(),
                Some(d) => format!("+{}", format_mmss(d)),
                None => "--:--".to_string(),
            };
            (row.display_name.clone(), status)
        })
        .collect();

    let name_width = cells
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, MAX_NAME_LEN);
    let status_width = cells
        .iter()
        .map(|(_, s)| s.chars().count())
        .max()
        .unwrap_or(7)
        .max(7);

    let sep = format!(
        "+{}+{}+\n",
        "-".repeat(name_width + 2),
        "-".repeat(status_width + 2)
    );
    let mut table = String::new();
    table.push_str(&sep);
    table.push_str(&format!(
        "| {:<nw$} | {:<sw$} |\n",
        "User",
        "Arrived",
        nw = name_width,
        sw = status_width
    ));
    table.push_str(&sep);
    for (name, status) in &cells {
        let trimmed: String = name.chars().take(name_width).collect();
        table.push_str(&format!(
            "| {:<nw$} | {:<sw$} |\n",
            trimmed,
            status,
            nw = name_width,
            sw = status_width
        ));
    }
    table.push_str(&sep);

    let elapsed = now.saturating_duration_since(started_at);
    let header = if in_grace {
        format!(
            "Grace period: **{}** remaining (counting starts at {}).",
            format_mmss(grace_remaining),
            format_mmss(GRACE_PERIOD)
        )
    } else {
        format!(
            "Counting since gather started — elapsed: **{}**.\nLate arrivals are stamped with their time-from-start.",
            format_mmss(elapsed)
        )
    };

    let present = cells.iter().filter(|(_, s)| s != "--:--").count();
    let total = cells.len();
    let ping_status = if silent { "🔕 off" } else { "🔔 on" };

    let color = if footer.is_some() {
        Color::DARK_GREEN
    } else if in_grace {
        Color::DARK_BLUE
    } else {
        Color::ORANGE
    };

    let mut builder = CreateEmbed::new()
        .color(color)
        .title("📣  Voice Channel Gathering")
        .description(format!(
            "{}\n\nGhost pings: {}\nAttendance: **{}/{}**\n```\n{}```",
            header, ping_status, present, total, table
        ));

    if let Some(text) = footer {
        builder = builder.footer(CreateEmbedFooter::new(text));
    }

    builder
}

pub fn pregather_buttons(disabled: bool) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(BTN_FORCE_START)
            .label("Start now")
            .style(ButtonStyle::Primary)
            .disabled(disabled),
        CreateButton::new(BTN_CANCEL)
            .label("Cancel")
            .style(ButtonStyle::Danger)
            .disabled(disabled),
    ])]
}

pub fn gather_buttons(
    disabled: bool,
    silent: bool,
) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(BTN_HERE)
            .label("I'm here!")
            .style(ButtonStyle::Success)
            .disabled(disabled),
        CreateButton::new(BTN_FORCE_START)
            .label("Force start")
            .style(ButtonStyle::Primary)
            .disabled(disabled),
        CreateButton::new(BTN_TOGGLE_SILENT)
            .label(if silent { "🔔 Unmute pings" } else { "🔕 Mute pings" })
            .style(ButtonStyle::Secondary)
            .disabled(disabled),
        CreateButton::new(BTN_CANCEL)
            .label("Cancel")
            .style(ButtonStyle::Danger)
            .disabled(disabled),
    ])]
}
