use chrono::{DateTime, Utc};

pub(crate) const PROJECT_COL_WIDTH: u16 = 15;

pub(crate) fn truncate_project(name: &str) -> String {
    let max = PROJECT_COL_WIDTH as usize;
    if name.chars().count() > max {
        let truncated: String = name.chars().take(max - 1).collect();
        format!("{truncated}…")
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

pub(crate) fn format_cost(tokens_in: Option<u64>, tokens_out: Option<u64>, model: &str) -> String {
    match (tokens_in, tokens_out) {
        (Some(i), Some(o)) => {
            let cost = consult_llm_core::llm_cost::calculate_cost(i, o, model);
            if cost.total_cost > 0.0 {
                format_cost_value(cost.total_cost)
            } else {
                "\u{2014}".to_string()
            }
        }
        _ => "\u{2014}".to_string(),
    }
}

pub(crate) fn format_cost_value(cost: f64) -> String {
    if cost >= 0.995 {
        format!("${:.2}", cost)
    } else if cost >= 0.0095 {
        format!("{:.0}\u{00a2}", cost * 100.0)
    } else if cost >= 0.00095 {
        format!("{:.1}\u{00a2}", cost * 100.0)
    } else {
        format!("{:.2}\u{00a2}", cost * 100.0)
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
