pub fn number_to_emoji(number: usize) -> String {
    let emoji_numbers = [
        ":zero:", ":one:", ":two:", ":three:", ":four:",
        ":five:", ":six:", ":seven:", ":eight:", ":nine:",
    ];

    number.to_string()
        .chars()
        .map(|c| emoji_numbers[c.to_digit(10).unwrap() as usize].to_string())
        .collect::<Vec<String>>()
        .join("")
}