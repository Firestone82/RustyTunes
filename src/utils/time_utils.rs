use regex::Regex;
use std::ops::Add;
use std::time::Duration;
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

#[derive(Debug, thiserror::Error)]
pub enum TimeParseError {
    #[error("Invalid time format")]
    InvalidTimeFormat,
}

/// Returns local time, switching between CET (UTC+1) and CEST (UTC+2) by month.
pub fn get_current_time() -> OffsetDateTime {
    let now_utc: OffsetDateTime = OffsetDateTime::now_utc();
    let current_month: u8 = now_utc.month() as u8;

    let utc_offset: UtcOffset = if (3..=10).contains(&current_month) {
        UtcOffset::from_whole_seconds(7200).unwrap() // UTC+2
    } else {
        UtcOffset::from_whole_seconds(3600).unwrap() // UTC+1
    };

    now_utc.to_offset(utc_offset)
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
        parts.push(format!(
            "{} {}",
            m,
            if m == 1 { "minute" } else { "minutes" }
        ));
    }
    if s > 0 {
        parts.push(format!(
            "{} {}",
            s,
            if s == 1 { "second" } else { "seconds" }
        ));
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

pub fn format_time(t: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year(),
        t.month() as u8,
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

fn parse_offset_secs(text: &str) -> Option<u64> {
    let re: Regex = Regex::new(r"^(?:(\d+)mo(?:nths?)?)?\s*(?:(\d+)\s*d(?:ays?)?)?\s*(?:(\d+)\s*h(?:ours?)?)?\s*(?:(\d+)\s*m(?:inutes?)?)?\s*(?:(\d+)\s*s(?:econds?)?)?$").unwrap();

    let captures = re.captures(text)?;

    let mut total_secs: u64 = 0;
    let mut matched_any = false;

    let units = [(1u64, 30 * 24 * 3600), (2, 24 * 3600), (3, 3600), (4, 60), (5, 1)];

    for (group, multiplier) in units {
        if let Some(m) = captures.get(group as usize) {
            let v: u64 = m.as_str().parse().unwrap_or(0);
            total_secs = total_secs.saturating_add(v.saturating_mul(multiplier));
            matched_any = true;
        }
    }

    if !matched_any || total_secs == 0 {
        return None;
    }

    Some(total_secs)
}

/// Parse a relative duration string (e.g. `"5m"`, `"1h 30s"`) into a `Duration`.
pub fn parse_duration_from_string(text: &str) -> Option<Duration> {
    let secs = parse_offset_secs(text)?;
    Some(Duration::from_secs(secs))
}

pub fn convert_time_offset_from_string(text: String) -> Option<OffsetDateTime> {
    let secs = parse_offset_secs(text.as_str())?;
    Some(get_current_time().add(Duration::from_secs(secs)))
}

pub fn convert_literal_from_string(text: String) -> Option<OffsetDateTime> {
    let now = get_current_time();
    match text.as_str() {
        "tomorrow" => Some(now.add(Duration::from_secs(24 * 60 * 60))),
        "week" => Some(now.add(Duration::from_secs(7 * 24 * 60 * 60))),
        _ => None,
    }
}

pub fn convert_time_date_from_string(text: String) -> Option<OffsetDateTime> {
    let local_offset = get_current_time().offset();

    let date_format = time::format_description::parse("[day]-[month]-[year]").unwrap();
    if let Ok(date) = Date::parse(&text, &date_format) {
        let naive = PrimitiveDateTime::new(date, Time::from_hms(9, 0, 0).unwrap());
        return Some(naive.assume_offset(local_offset));
    }

    let datetime_format = time::format_description::parse("[day]-[month]-[year]_[hour]:[minute]").unwrap();
    if let Ok(datetime) = PrimitiveDateTime::parse(&text, &datetime_format) {
        return Some(datetime.assume_offset(local_offset));
    }

    None
}

pub fn parse_text(text: String) -> Result<OffsetDateTime, TimeParseError> {
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err(TimeParseError::InvalidTimeFormat);
    }

    convert_literal_from_string(trimmed.clone())
        .or_else(|| convert_time_date_from_string(trimmed.clone()))
        .or_else(|| convert_time_offset_from_string(trimmed.clone()))
        .ok_or(TimeParseError::InvalidTimeFormat)
}
