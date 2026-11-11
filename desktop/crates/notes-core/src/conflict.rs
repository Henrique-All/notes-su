use chrono::{DateTime, Utc};

pub const CONFLICT_TIME_THRESHOLD_MS: i64 = 60_000;

pub fn normalize_markdown_for_conflict(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").trim().to_owned()
}

pub fn is_content_conflicted(
    local_data: &str,
    local_locked: bool,
    local_date_edited: DateTime<Utc>,
    local_is_unsynced: bool,
    remote_data: &str,
    remote_locked: bool,
    remote_date_edited: DateTime<Utc>,
) -> bool {
    if !local_is_unsynced {
        return false;
    }
    if local_locked != remote_locked {
        return true;
    }
    let delta_ms = (remote_date_edited.timestamp_millis() - local_date_edited.timestamp_millis()).abs();
    if delta_ms < CONFLICT_TIME_THRESHOLD_MS {
        return false;
    }
    if local_locked {
        return local_data.trim() != remote_data.trim();
    }
    normalize_markdown_for_conflict(local_data) != normalize_markdown_for_conflict(remote_data)
}
