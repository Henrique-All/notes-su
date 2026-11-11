use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
use serde_json::Value;
use uuid::Uuid;

use crate::models::{CipherEnvelope, ConflictedContent, ContentFormat, Tag};

pub fn parse_datetime_value(val: &Value) -> Result<DateTime<Utc>, String> {
    match val {
        Value::String(s) => parse_datetime_str(s),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(epoch_to_utc(i))
            } else if let Some(f) = n.as_f64() {
                Ok(epoch_to_utc(f as i64))
            } else {
                Err(format!("invalid numeric timestamp: {n}"))
            }
        }
        other => Err(format!("expected string or numeric timestamp, got {other}")),
    }
}

pub fn parse_datetime_str(s: &str) -> Result<DateTime<Utc>, String> {
    let trimmed = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            return Ok(ndt.and_utc());
        }
    }
    if let Ok(nd) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        && let Some(ndt) = nd.and_hms_opt(0, 0, 0)
    {
        return Ok(ndt.and_utc());
    }
    if let Ok(epoch) = trimmed.parse::<i64>() {
        return Ok(epoch_to_utc(epoch));
    }
    Err(format!("invalid timestamp format: '{trimmed}'"))
}

fn epoch_to_utc(raw: i64) -> DateTime<Utc> {
    if raw.abs() >= 100_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(raw).unwrap_or_else(Utc::now)
    } else {
        DateTime::<Utc>::from_timestamp(raw, 0).unwrap_or_else(Utc::now)
    }
}

pub mod flexible_datetime {
    use super::*;

    pub fn serialize<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&dt.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = Value::deserialize(deserializer)?;
        parse_datetime_value(&val).map_err(D::Error::custom)
    }
}

pub mod flexible_opt_datetime {
    use super::*;

    pub fn serialize<S>(opt: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match opt {
            Some(dt) => serializer.serialize_some(&dt.to_rfc3339_opts(SecondsFormat::Millis, true)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<Value>::deserialize(deserializer)?;
        match opt {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
            Some(v) => parse_datetime_value(&v).map(Some).map_err(D::Error::custom),
        }
    }
}

pub fn envelope_or_json_str<'de, D>(deserializer: D) -> Result<CipherEnvelope, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Value::deserialize(deserializer)?;
    match val {
        Value::String(s) => serde_json::from_str::<CipherEnvelope>(&s).map_err(D::Error::custom),
        other => serde_json::from_value::<CipherEnvelope>(other).map_err(D::Error::custom),
    }
}

pub fn opt_envelope_or_json_str<'de, D>(deserializer: D) -> Result<Option<CipherEnvelope>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Value>::deserialize(deserializer)?;
    match opt {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => serde_json::from_str::<CipherEnvelope>(&s)
            .map(Some)
            .map_err(D::Error::custom),
        Some(other) => serde_json::from_value::<CipherEnvelope>(other)
            .map(Some)
            .map_err(D::Error::custom),
    }
}

pub fn opt_conflicted_or_json_str<'de, D>(
    deserializer: D,
) -> Result<Option<ConflictedContent>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Value>::deserialize(deserializer)?;
    match opt {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            if let Ok(parsed) = serde_json::from_str::<ConflictedContent>(&s) {
                Ok(Some(parsed))
            } else {
                let now = Utc::now();
                Ok(Some(ConflictedContent {
                    data: s,
                    locked: false,
                    format: ContentFormat::Markdown,
                    date_edited: now,
                    date_modified: now,
                    device_id: None,
                }))
            }
        }
        Some(other) => serde_json::from_value::<ConflictedContent>(other)
            .map(Some)
            .map_err(D::Error::custom),
    }
}

pub fn tags_flexible<'de, D>(deserializer: D) -> Result<Vec<Tag>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Vec<Value>>::deserialize(deserializer)?;
    let Some(items) = opt else {
        return Ok(Vec::new());
    };
    let mut tags = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::String(title) => {
                let now = Utc::now();
                let id = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("tag:{title}").as_bytes())
                    .to_string();
                tags.push(Tag {
                    id,
                    user_id: None,
                    title,
                    note_count: None,
                    date_created: now,
                    date_modified: now,
                    deleted: false,
                    revision: 0,
                });
            }
            other => {
                let tag = serde_json::from_value::<Tag>(other).map_err(D::Error::custom)?;
                tags.push(tag);
            }
        }
    }
    Ok(tags)
}
