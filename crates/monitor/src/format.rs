use chrono::{DateTime, Utc};

pub(crate) const PROJECT_COL_WIDTH: u16 = 15;

pub(crate) fn truncate_project(name: &str) -> String {
    if name.len() > PROJECT_COL_WIDTH as usize {
        format!("{}…", &name[..PROJECT_COL_WIDTH as usize - 1])
    } else {
        name.to_string()
    }
}

pub(crate) fn format_duration_friendly(ms: u64) -> String {
    let secs = ms as f64 / 1000.0;
    if secs >= 60.0 {
        let m = secs as u64 / 60;
        let s = secs as u64 % 60;
        format!("{m}m {s}s")
    } else {
        format!("{secs:.1}s")
    }
}

pub(crate) fn format_relative_time(parsed_ts: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(parsed) = parsed_ts else {
        return "\u{2014}".to_string();
    };
    let secs = now.signed_duration_since(parsed).num_seconds().max(0);

    if secs < 10 {
        "just now".to_string()
    } else if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

pub(crate) fn format_tokens(tokens_in: Option<u64>, tokens_out: Option<u64>) -> String {
    match (tokens_in, tokens_out) {
        (Some(i), Some(o)) => {
            format!("{}/{}", format_token_count(i), format_token_count(o))
        }
        _ => "\u{2014}".to_string(),
    }
}

pub(crate) fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        let m = n as f64 / 1_000_000.0;
        if n >= 100_000_000 {
            format!("{:.0}M", m)
        } else if n >= 10_000_000 {
            format!("{:.1}M", m)
        } else {
            format!("{:.2}M", m)
        }
    } else if n >= 1_000 {
        let k = n as f64 / 1_000.0;
        if n >= 100_000 {
            format!("{:.0}k", k)
        } else if n >= 10_000 {
            format!("{:.1}k", k)
        } else {
            format!("{:.2}k", k)
        }
    } else {
        n.to_string()
    }
}
