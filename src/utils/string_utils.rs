use unicode_segmentation::UnicodeSegmentation;

pub const MAX_NAME_LEN: usize = 21;

pub fn number_to_emoji(number: usize) -> String {
    let emoji_numbers = [":zero:", ":one:", ":two:", ":three:", ":four:", ":five:", ":six:", ":seven:", ":eight:", ":nine:"];

    number
        .to_string()
        .chars()
        .map(|c| emoji_numbers[c.to_digit(10).unwrap() as usize].to_string())
        .collect::<Vec<String>>()
        .join("")
}

/// Replace emoji grapheme clusters with their `:shortcode:` then truncate to
/// `MAX_NAME_LEN` so embed tables stay aligned in Discord's monospace font.
pub fn sanitize_name(name: &str) -> String {
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
