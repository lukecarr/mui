pub mod screens;
pub mod theme;
pub mod widgets;

/// Format an RFC 3339 last-played timestamp into a human-readable relative string.
///
/// Returns "Never" for `None`, a relative phrase like "3 hours ago" for recent
/// timestamps, and a date like "Mar 5, 2026" for anything older than 7 days.
/// Falls back to the raw string if parsing fails.
pub fn format_last_played(rfc3339: Option<&str>) -> String {
    let Some(raw) = rfc3339 else {
        return "Never".to_string();
    };

    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) else {
        return raw.to_string();
    };

    let now = chrono::Utc::now();
    let elapsed = now.signed_duration_since(dt);

    if elapsed.num_seconds() < 60 {
        "Just now".to_string()
    } else if elapsed.num_minutes() < 60 {
        let m = elapsed.num_minutes();
        if m == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{m} minutes ago")
        }
    } else if elapsed.num_hours() < 24 {
        let h = elapsed.num_hours();
        if h == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{h} hours ago")
        }
    } else if elapsed.num_days() < 7 {
        let d = elapsed.num_days();
        if d == 1 {
            "1 day ago".to_string()
        } else {
            format!("{d} days ago")
        }
    } else {
        dt.format("%b %-d, %Y").to_string()
    }
}
