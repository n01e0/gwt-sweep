use std::time::Duration;

pub fn parse_duration_spec(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("duration must not be empty".to_owned());
    }

    let split_at = raw
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(raw.len());
    if split_at == 0 {
        return Err("duration must start with a number".to_owned());
    }

    let value = raw[..split_at]
        .parse::<u64>()
        .map_err(|error| format!("invalid duration number: {error}"))?;
    let suffix = raw[split_at..].trim().to_ascii_lowercase();
    let multiplier = match suffix.as_str() {
        "" | "d" | "day" | "days" => 86_400,
        "w" | "week" | "weeks" => 604_800,
        "h" | "hr" | "hour" | "hours" => 3_600,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "s" | "sec" | "secs" | "second" | "seconds" => 1,
        _ => {
            return Err(
                "duration suffix must be one of s, m, h, d, or w; bare numbers mean days"
                    .to_owned(),
            );
        }
    };

    let seconds = value
        .checked_mul(multiplier)
        .ok_or_else(|| "duration is too large".to_owned())?;
    Ok(Duration::from_secs(seconds))
}

pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds.is_multiple_of(604_800) {
        format!("{}w", seconds / 604_800)
    } else if seconds.is_multiple_of(86_400) {
        format!("{}d", seconds / 86_400)
    } else if seconds.is_multiple_of(3_600) {
        format!("{}h", seconds / 3_600)
    } else if seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_duration_specs() {
        assert_eq!(
            parse_duration_spec("3").unwrap(),
            Duration::from_secs(259_200)
        );
        assert_eq!(
            parse_duration_spec("2w").unwrap(),
            Duration::from_secs(1_209_600)
        );
        assert_eq!(
            parse_duration_spec("12h").unwrap(),
            Duration::from_secs(43_200)
        );
        assert!(parse_duration_spec("hours").is_err());
    }
}
