use std::time::Duration;
use time::OffsetDateTime;

pub const MAX_NAME_LEN: usize = 21;

pub fn number_to_emoji(number: usize) -> String {
    let emoji_numbers = [
        ":zero:", ":one:", ":two:", ":three:", ":four:", ":five:", ":six:", ":seven:", ":eight:",
        ":nine:",
    ];

    number
        .to_string()
        .chars()
        .map(|c| emoji_numbers[c.to_digit(10).unwrap() as usize].to_string())
        .collect::<Vec<String>>()
        .join("")
}

pub fn humanize_duration(d: Duration) -> String {
    let total = d.as_secs();
    if total == 0 {
        return "0 seconds".to_string();
    }
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    let mut parts: Vec<String> = Vec::new();
    if h > 0 {
        parts.push(format!("{} {}", h, if h == 1 { "hour" } else { "hours" }));
    }
    if m > 0 {
        parts.push(format!("{} {}", m, if m == 1 { "minute" } else { "minutes" }));
    }
    if s > 0 {
        parts.push(format!("{} {}", s, if s == 1 { "second" } else { "seconds" }));
    }
    parts.join(" ")
}

pub fn format_wall_clock(t: OffsetDateTime) -> String {
    format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second())
}

pub fn format_mmss(d: Duration) -> String {
    let total = d.as_secs();
    let m = total / 60;
    let s = total % 60;
    format!("{:02}:{:02}", m, s)
}

/// Replace emoji grapheme clusters with their `:shortcode:` then truncate to
/// `MAX_NAME_LEN` so embed tables stay aligned in Discord's monospace font.
pub fn sanitize_name(name: &str) -> String {
    use unicode_segmentation::UnicodeSegmentation;

    let mut out = String::new();
    for g in name.graphemes(true) {
        if let Some(emoji) = emojis::get(g) {
            let label = emoji.shortcode().unwrap_or(emoji.name());
            out.push(':');
            out.push_str(label.trim_matches(':'));
            out.push(':');
        } else {
            out.push_str(g);
        }
    }

    out.chars().take(MAX_NAME_LEN).collect()
}
