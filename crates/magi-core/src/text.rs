//! Small formatting helpers shared by node summaries and the UI.

/// Formats a duration in milliseconds the way a person would say it:
/// `250 ms`, `1.5 s`, `2 min 5 s`, `1 h 3 min`.
pub fn format_ms(ms: f64) -> String {
    let ms = ms.max(0.0);
    if ms < 1000.0 {
        return format!("{} ms", trim_float(ms, 1));
    }
    let secs = ms / 1000.0;
    if secs < 60.0 {
        return format!("{} s", trim_float(secs, 2));
    }
    let total_secs = secs.round() as u64;
    let (hours, mins, secs) = (total_secs / 3600, total_secs % 3600 / 60, total_secs % 60);
    match (hours, mins, secs) {
        (0, m, 0) => format!("{m} min"),
        (0, m, s) => format!("{m} min {s} s"),
        (h, 0, _) => format!("{h} h"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

/// Formats a number with up to `decimals` decimals and no trailing zeros.
pub fn trim_float(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
}

/// Parses `#RRGGBB` (the leading `#` is optional).
pub fn parse_hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

pub fn format_hex_color(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_ms(0.0), "0 ms");
        assert_eq!(format_ms(12.5), "12.5 ms");
        assert_eq!(format_ms(1500.0), "1.5 s");
        assert_eq!(format_ms(60_000.0), "1 min");
        assert_eq!(format_ms(125_000.0), "2 min 5 s");
        assert_eq!(format_ms(3_780_000.0), "1 h 3 min");
    }

    #[test]
    fn colors_round_trip() {
        assert_eq!(parse_hex_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(format_hex_color([255, 128, 0]), "#FF8000");
        assert_eq!(parse_hex_color("nope"), None);
    }
}
