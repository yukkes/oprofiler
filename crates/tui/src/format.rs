pub fn format_count(value: u64) -> String {
    let s = value.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

pub fn format_signed_count(value: i64) -> String {
    if value >= 0 {
        format!("+{}", format_count(value as u64))
    } else {
        format!("-{}", format_count(value.unsigned_abs()))
    }
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{} kB", format_count((bytes + 1023) / 1024))
    } else {
        format!("{} bytes", format_count(bytes))
    }
}

pub fn format_duration_ms(ms: f64) -> String {
    format_duration_nano((ms * 1_000_000.0) as u64)
}

pub fn format_duration_nano(nanos: u64) -> String {
    if nanos >= 60_000_000_000 {
        format!("{:.0} m", nanos as f64 / 60_000_000_000.0)
    } else if nanos >= 1_000_000_000 {
        format!("{:.0} s", nanos as f64 / 1_000_000_000.0)
    } else if nanos >= 1_000_000 {
        format!("{} ms", format_count(nanos / 1_000_000))
    } else if nanos >= 1_000 {
        format!("{} us", format_count(nanos / 1_000))
    } else {
        format!("{} ns", format_count(nanos))
    }
}

pub fn format_duration_nano_f(nanos: f64) -> String {
    if nanos >= 60_000_000_000.0 {
        format!("{:.0} m", nanos / 60_000_000_000.0)
    } else if nanos >= 1_000_000_000.0 {
        format!("{:.0} s", nanos / 1_000_000_000.0)
    } else if nanos >= 1_000_000.0 {
        format!("{:.0} ms", nanos / 1_000_000.0)
    } else if nanos >= 1_000.0 {
        format!("{:.0} us", nanos / 1_000.0)
    } else {
        format!("{:.0} ns", nanos)
    }
}

pub fn format_signed_bytes(bytes: i64) -> String {
    if bytes >= 0 {
        format!("+{}", format_bytes(bytes as u64))
    } else {
        format!("-{}", format_bytes(bytes.unsigned_abs()))
    }
}

pub fn short_method(method: &str) -> String {
    let compact = method.rsplit('/').next().unwrap_or(method);
    let compact: String = compact
        .rsplit('.')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(".");
    compact
}

pub fn truncate_middle(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let head = max_chars / 2;
    let tail = max_chars.saturating_sub(head + 3);
    let start: String = text.chars().take(head).collect();
    let end: String = text
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{start}...{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_nano_f_microsecond_precision() {
        // 1 us
        assert_eq!(format_duration_nano_f(1_000.0), "1 us");
        // 999 us
        assert_eq!(format_duration_nano_f(999_000.0), "999 us");
        // 1 ms boundary (should switch to ms)
        assert_eq!(format_duration_nano_f(1_000_000.0), "1 ms");
        // 500 us
        assert_eq!(format_duration_nano_f(500_000.0), "500 us");
        // 1 ns boundary
        assert_eq!(format_duration_nano_f(1.0), "1 ns");
        // 999 ns
        assert_eq!(format_duration_nano_f(999.0), "999 ns");
    }

    #[test]
    fn test_format_duration_nano_f_hot_spot_values() {
        // Typical hot spot values that should show in microseconds
        // 150 us
        assert_eq!(format_duration_nano_f(150_000.0), "150 us");
        // 1.5 ms -> should show as ms since >= 1_000_000
        assert_eq!(format_duration_nano_f(1_500_000.0), "2 ms");
        // 10 us
        assert_eq!(format_duration_nano_f(10_000.0), "10 us");
    }
}
