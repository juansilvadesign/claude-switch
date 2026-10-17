//! Antigravity quota saved by the in-Antigravity status line. Reads never touch `.gemini`.

use crate::atomic;
use crate::limits::{Limits, Snapshot, Window, WindowGroup};
use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_ACCOUNT: &str = "antigravity:default";
const BUCKETS: [(&str, WindowGroup, &str); 4] = [
    ("gemini-5h", WindowGroup::Session, "session"),
    ("gemini-weekly", WindowGroup::Weekly, "weekly_all"),
    ("3p-5h", WindowGroup::Other(String::new()), "3p_5h"),
    ("3p-weekly", WindowGroup::Other(String::new()), "3p_weekly"),
];
const MAX_AGE: Duration = Duration::hours(24);

pub fn valid_account(account: &str) -> bool {
    account == DEFAULT_ACCOUNT || crate::profile::safe_shell_name(account)
}

pub fn path(base: &Path, account: &str) -> Option<PathBuf> {
    valid_account(account).then(|| base.join("agy-quota").join(format!("{account}.json")))
}

fn parsed_bucket<'a>(quota: &'a Value, id: &str) -> Option<&'a Value> {
    let bucket = quota.get(id)?;
    let fraction = bucket.get("remaining_fraction")?.as_f64()?;
    if !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    DateTime::parse_from_rfc3339(bucket.get("reset_time")?.as_str()?).ok()?;
    Some(bucket)
}

fn validated_quota(quota: &Value) -> Option<Value> {
    let mut selected = serde_json::Map::new();
    for (id, _, _) in &BUCKETS {
        let bucket = parsed_bucket(quota, id)?;
        selected.insert(
            (*id).into(),
            json!({
                "remaining_fraction": bucket.get("remaining_fraction")?,
                "reset_time": bucket.get("reset_time")?
            }),
        );
    }
    Some(Value::Object(selected))
}

pub fn save(
    base: &Path,
    account: &str,
    quota: &Value,
    plan_tier: Option<&str>,
    now: DateTime<Utc>,
) -> Result<bool> {
    let Some(path) = path(base, account) else {
        bail!("Invalid Antigravity account key");
    };
    let Some(quota) = validated_quota(quota) else {
        return Ok(false);
    };
    let value = json!({"fetched_at": now.to_rfc3339(), "quota": quota,
        "plan_tier": plan_tier.filter(|tier| !tier.trim().is_empty())});
    atomic::write_private(&path, &serde_json::to_vec(&value)?)?;
    Ok(true)
}

fn saved(base: &Path, account: &str, now: DateTime<Utc>) -> Option<Value> {
    let path = path(base, account)?;
    let bytes = fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let fetched = DateTime::parse_from_rfc3339(value.get("fetched_at")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    if fetched > now + Duration::minutes(5) || now - fetched > MAX_AGE {
        return None;
    }
    Some(value)
}

pub fn read_limits(base: &Path, account: &str, now: DateTime<Utc>) -> Limits {
    let Some(value) = saved(base, account, now) else {
        return Limits::NoSnapshot;
    };
    let Some(quota) = value.get("quota") else {
        return Limits::NoSnapshot;
    };
    let fetched_at = DateTime::parse_from_rfc3339(value["fetched_at"].as_str().unwrap_or_default())
        .map(|time| time.with_timezone(&Utc));
    match fetched_at.ok().and_then(|at| snapshot(quota, at)) {
        Some(snapshot) => Limits::Snapshot(snapshot),
        None => Limits::NoSnapshot,
    }
}

pub fn snapshot(quota: &Value, fetched_at: DateTime<Utc>) -> Option<Snapshot> {
    let mut windows = Vec::new();
    for (id, group, kind) in &BUCKETS {
        let bucket = parsed_bucket(quota, id)?;
        let fraction = bucket["remaining_fraction"].as_f64().unwrap_or(0.0);
        let reset = bucket["reset_time"].as_str().and_then(|raw| {
            DateTime::parse_from_rfc3339(raw)
                .ok()
                .map(|time| time.with_timezone(&Utc))
        });
        windows.push(Window {
            group: group.clone(),
            kind: (*kind).into(),
            percent: (1.0 - fraction) * 100.0,
            severity: None,
            resets_at: reset,
            is_active: None,
        });
    }
    Some(Snapshot {
        fetched_at,
        windows,
        window_started_at: None,
    })
}

pub fn plan_tier(base: &Path, account: &str, now: DateTime<Utc>) -> Option<String> {
    saved(base, account, now)?
        .get("plan_tier")?
        .as_str()
        .map(str::to_string)
}

pub fn quota_from_refresh(value: &Value) -> Option<Value> {
    let groups = value
        .get("command")?
        .get("data")?
        .get("groups")?
        .as_array()?;
    let mut quota = serde_json::Map::new();
    for bucket in groups
        .iter()
        .filter_map(|group| group.get("buckets").and_then(Value::as_array))
        .flatten()
    {
        let Some(id) = bucket.get("id").and_then(Value::as_str) else {
            continue;
        };
        if BUCKETS.iter().any(|(expected, _, _)| *expected == id) {
            quota.insert(id.into(), bucket.clone());
        }
    }
    validated_quota(&Value::Object(quota))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::TempDir;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2030, 1, 1, 12, 0, 0).unwrap()
    }

    fn quota() -> Value {
        json!({
            "gemini-5h":{"remaining_fraction":0.88,"reset_time":"2030-01-01T15:00:00Z","reset_in_seconds":1},
            "gemini-weekly":{"remaining_fraction":0.65,"reset_time":"2030-01-07T12:00:00Z"},
            "3p-5h":{"remaining_fraction":0.99,"reset_time":"2030-01-01T16:00:00Z"},
            "3p-weekly":{"remaining_fraction":0.98,"reset_time":"2030-01-08T12:00:00Z"}
        })
    }

    #[test]
    fn remaining_fraction_is_inverted_and_gemini_sets_headline_windows() {
        // Known-bad: rf*100, swapped windows, or third-party buckets as headline bars.
        let temp = TempDir::new().unwrap();
        assert!(save(temp.path(), "spare", &quota(), Some("Google AI Pro"), now()).unwrap());
        let Limits::Snapshot(snapshot) = read_limits(temp.path(), "spare", now()) else {
            panic!("expected snapshot")
        };
        assert!((snapshot.session().unwrap().percent - 12.0).abs() < 0.0001);
        assert!((snapshot.weekly().unwrap().percent - 35.0).abs() < 0.0001);
        assert_eq!(
            snapshot.session().unwrap().resets_at.unwrap().to_rfc3339(),
            "2030-01-01T15:00:00+00:00"
        );
        assert_eq!(snapshot.windows[2].group, WindowGroup::Other(String::new()));
        assert_eq!(snapshot.windows[3].group, WindowGroup::Other(String::new()));
        assert_eq!(snapshot.age(now() + Duration::hours(2)), "2 h ago");
        let saved: Value =
            serde_json::from_slice(&fs::read(path(temp.path(), "spare").unwrap()).unwrap())
                .unwrap();
        assert!(saved.get("email").is_none());
        assert_eq!(saved["quota"].as_object().unwrap().len(), 4);
        assert!(
            saved["quota"]["gemini-5h"]
                .get("reset_in_seconds")
                .is_none()
        );
        assert_eq!(
            plan_tier(temp.path(), "spare", now()).as_deref(),
            Some("Google AI Pro")
        );
    }

    #[test]
    fn missing_unreadable_and_stale_saved_quota_are_no_snapshot() {
        // Known-bad: absent quota becomes 0%, or an old snapshot remains eligible.
        let temp = TempDir::new().unwrap();
        assert_eq!(read_limits(temp.path(), "spare", now()), Limits::NoSnapshot);
        save(temp.path(), "spare", &quota(), None, now()).unwrap();
        assert_eq!(
            read_limits(temp.path(), "spare", now() + Duration::hours(25)),
            Limits::NoSnapshot
        );
        fs::write(path(temp.path(), "spare").unwrap(), b"bad").unwrap();
        assert_eq!(read_limits(temp.path(), "spare", now()), Limits::NoSnapshot);
        assert_ne!(
            path(temp.path(), "spare"),
            path(temp.path(), DEFAULT_ACCOUNT)
        );
    }

    #[test]
    fn live_refresh_envelope_selects_four_known_buckets() {
        let buckets = quota();
        let input = json!({"command":{"data":{"groups":[
            {"buckets":[{"id":"gemini-5h","remaining_fraction":0.88,"reset_time":"2030-01-01T15:00:00Z"},{"id":"gemini-weekly","remaining_fraction":0.65,"reset_time":"2030-01-07T12:00:00Z"}]},
            {"buckets":[{"id":"3p-5h","remaining_fraction":0.99,"reset_time":"2030-01-01T16:00:00Z"},{"id":"3p-weekly","remaining_fraction":0.98,"reset_time":"2030-01-08T12:00:00Z"}]}
        ]}}});
        assert_eq!(
            quota_from_refresh(&input).unwrap()["gemini-5h"]["remaining_fraction"],
            buckets["gemini-5h"]["remaining_fraction"]
        );
    }
}
