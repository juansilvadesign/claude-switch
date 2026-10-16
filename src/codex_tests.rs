use crate::codex::{read_limits, read_stats, reset_read_stats};
use crate::limits::{LimitSource, Limits, Snapshot, format_info};
use crate::profile::{Profile, ProfileManager, Registry, Tool};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

const ACCOUNT: &str = "00000000-0000-4000-8000-000000000001";
const OTHER: &str = "00000000-0000-4000-8000-000000000002";
type FileState = (PathBuf, Option<(Vec<u8>, std::time::SystemTime)>);

pub(crate) fn at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .unwrap()
        .with_timezone(&Utc)
}

pub(crate) fn window(minutes: i64, percent: Value, reset: Option<i64>) -> Value {
    json!({"window_minutes": minutes, "used_percent": percent, "resets_at": reset})
}

pub(crate) fn event(at: &str, limit_id: &str, primary: Value, secondary: Value) -> Value {
    json!({
        "timestamp": at, "type": "event_msg",
        "payload": {"type": "token_count", "info": null,
            "rate_limits": {"limit_id": limit_id, "primary": primary,
                "secondary": secondary, "plan_type": "plus"}}
    })
}

pub(crate) fn modified(path: &Path, at_time: &str) {
    File::open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(at(at_time).timestamp() as u64))
        .unwrap();
}

pub(crate) fn session(
    home: &Path,
    number: u8,
    creator: Option<&str>,
    lines: &[Value],
    mtime: &str,
) -> PathBuf {
    let dir = home.join("sessions/2030/01/01");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!(
        "rollout-2030-01-01T00-00-{number:02}-00000000-0000-4000-8000-000000000000.jsonl"
    ));
    let mut file = File::create(&path).unwrap();
    writeln!(
        file,
        "{}",
        json!({"timestamp":"2030-01-01T00:00:00Z",
            "type":"session_meta", "payload":{
                "id":"00000000-0000-4000-8000-000000000000",
                "creator_account_id":creator}})
    )
    .unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
    drop(file);
    modified(&path, mtime);
    path
}

fn snapshot(limits: Limits) -> Snapshot {
    let Limits::Snapshot(snapshot) = limits else {
        panic!("expected snapshot, got {limits:?}");
    };
    snapshot
}

#[test]
fn codex_windows_are_matched_by_length() {
    // Known-bad: primary is always treated as the five-hour window.
    let temp = TempDir::new().unwrap();
    let home = temp.path();
    let five = window(300, json!(12), None);
    let week = window(10_080, json!(26), None);
    let path = session(
        home,
        1,
        None,
        &[event(
            "2030-01-01T01:00:00Z",
            "codex",
            five.clone(),
            week.clone(),
        )],
        "2030-01-01T02:00:00Z",
    );
    let result = snapshot(read_limits(home, None));
    assert_eq!(result.session().unwrap().percent, 12.0);
    assert_eq!(result.weekly().unwrap().percent, 26.0);
    fs::remove_file(path).unwrap();
    let path = session(
        home,
        2,
        None,
        &[event("2030-01-01T01:00:00Z", "codex", week, Value::Null)],
        "2030-01-01T02:00:00Z",
    );
    let result = snapshot(read_limits(home, None));
    assert!(result.session().is_none());
    assert_eq!(result.weekly().unwrap().percent, 26.0);
    assert_eq!(
        crate::list_window(result.session(), at("2030-01-01T01:30:00Z"), false).0,
        "—"
    );
    fs::remove_file(path).unwrap();
    session(
        home,
        3,
        None,
        &[event("2030-01-01T01:00:00Z", "codex", Value::Null, five)],
        "2030-01-01T02:00:00Z",
    );
    let result = snapshot(read_limits(home, None));
    assert_eq!(result.session().unwrap().percent, 12.0);
    assert!(result.weekly().is_none());
}

#[test]
fn codex_newest_event_uses_mtime_and_skips_premium() {
    // Known-bads: sorting by filename, or accepting the last premium event.
    let temp = TempDir::new().unwrap();
    session(
        temp.path(),
        9,
        None,
        &[event(
            "2030-01-01T01:00:00Z",
            "codex",
            window(300, json!(70), None),
            Value::Null,
        )],
        "2030-01-03T00:00:00Z",
    );
    session(
        temp.path(),
        1,
        None,
        &[
            event(
                "2030-01-02T01:00:00Z",
                "codex",
                window(300, json!(12), None),
                Value::Null,
            ),
            event("2030-01-02T02:00:00Z", "premium", Value::Null, Value::Null),
        ],
        "2030-01-04T00:00:00Z",
    );
    let result = snapshot(read_limits(temp.path(), None));
    assert_eq!(result.fetched_at, at("2030-01-02T01:00:00Z"));
    assert_eq!(result.session().unwrap().percent, 12.0);
}

#[test]
fn codex_stops_before_opening_older_files() {
    // Known-bad: every session file is opened after the best event is found.
    let temp = TempDir::new().unwrap();
    session(
        temp.path(),
        1,
        None,
        &[event(
            "2030-01-02T12:00:00Z",
            "codex",
            window(300, json!(12), None),
            Value::Null,
        )],
        "2030-01-03T00:00:00Z",
    );
    session(
        temp.path(),
        2,
        None,
        &[event(
            "2030-01-01T12:00:00Z",
            "codex",
            window(300, json!(50), None),
            Value::Null,
        )],
        "2030-01-02T00:00:00Z",
    );
    reset_read_stats();
    assert_eq!(
        snapshot(read_limits(temp.path(), None))
            .session()
            .unwrap()
            .percent,
        12.0
    );
    assert_eq!(read_stats(), 1);
}

#[test]
fn codex_reads_only_the_last_megabyte() {
    // Known-bad: reading the whole file finds an event outside its tail.
    let temp = TempDir::new().unwrap();
    let newer = session(
        temp.path(),
        1,
        None,
        &[event(
            "2030-01-02T12:00:00Z",
            "codex",
            window(300, json!(12), None),
            Value::Null,
        )],
        "2030-01-04T00:00:00Z",
    );
    writeln!(
        OpenOptions::new().append(true).open(&newer).unwrap(),
        "{}",
        "x".repeat(2 * 1024 * 1024)
    )
    .unwrap();
    modified(&newer, "2030-01-04T00:00:00Z");
    session(
        temp.path(),
        2,
        None,
        &[event(
            "2030-01-01T12:00:00Z",
            "codex",
            window(300, json!(44), None),
            Value::Null,
        )],
        "2030-01-03T00:00:00Z",
    );
    assert_eq!(
        snapshot(read_limits(temp.path(), None))
            .session()
            .unwrap()
            .percent,
        44.0
    );
}

#[test]
fn codex_account_match_needs_both_ids() {
    // Known-bad: a missing id on either side hides the report.
    let temp = TempDir::new().unwrap();
    let path = session(
        temp.path(),
        1,
        Some(OTHER),
        &[event(
            "2030-01-01T01:00:00Z",
            "codex",
            window(300, json!(42), None),
            Value::Null,
        )],
        "2030-01-01T02:00:00Z",
    );
    let mismatch = read_limits(temp.path(), Some(ACCOUNT));
    assert_eq!(mismatch, Limits::AccountMismatch);
    let text = format_info(&mismatch, at("2030-01-01T01:30:00Z"), LimitSource::Codex);
    assert_eq!(
        text,
        "Plan limits: the last report belongs to another account, so it's hidden.\n"
    );
    assert!(!text.contains("42%") && !text.contains(ACCOUNT) && !text.contains(OTHER));
    assert!(matches!(
        read_limits(temp.path(), None),
        Limits::Snapshot(_)
    ));
    fs::remove_file(path).unwrap();
    session(
        temp.path(),
        2,
        None,
        &[event(
            "2030-01-01T01:00:00Z",
            "codex",
            window(300, json!(42), None),
            Value::Null,
        )],
        "2030-01-01T02:00:00Z",
    );
    assert!(matches!(
        read_limits(temp.path(), Some(ACCOUNT)),
        Limits::Snapshot(_)
    ));

    let manager =
        ProfileManager::with_paths(temp.path().join("switch"), temp.path().join(".claude"))
            .unwrap();
    let profile_dir = manager.profile_dir("spare-gpt");
    fs::create_dir_all(&profile_dir).unwrap();
    let mut registry = Registry::default();
    registry.profiles.insert(
        "spare-gpt".into(),
        Profile {
            name: "spare-gpt".into(),
            tool: Tool::Codex,
            email: Some("spare-gpt@example.com".into()),
            added: at("2030-01-01T00:00:00Z"),
            last_used: None,
        },
    );
    fs::write(
        manager.base_dir.join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    let payload = "eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiMDAwMDAwMDAtMDAwMC00MDAwLTgwMDAtMDAwMDAwMDAwMDAxIn19";
    fs::write(
        profile_dir.join("auth.json"),
        format!(r#"{{"tokens":{{"id_token":"h.{payload}.s"}}}}"#),
    )
    .unwrap();
    session(
        &profile_dir,
        1,
        Some(OTHER),
        &[event(
            "2030-01-01T01:00:00Z",
            "codex",
            window(300, json!(42), None),
            Value::Null,
        )],
        "2030-01-01T02:00:00Z",
    );
    let info = crate::info_output(&manager, "spare-gpt", at("2030-01-01T01:30:00Z")).unwrap();
    assert!(
        info.contains("Plan limits: the last report belongs to another account, so it's hidden.")
    );
    assert!(!info.contains("42%") && !info.contains(ACCOUNT) && !info.contains(OTHER));
}

#[test]
fn codex_bad_lines_do_not_break_the_read() {
    // Known-bad: one malformed or oversized line returns Unreadable.
    let temp = TempDir::new().unwrap();
    let path = session(temp.path(), 1, None, &[], "2030-01-01T02:00:00Z");
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file, "{}", "x".repeat(2 * 1024 * 1024)).unwrap();
    writeln!(
        file,
        "{}",
        event(
            "2030-01-01T01:00:00Z",
            "codex",
            window(300, json!(42), None),
            window(10_080, json!("bad"), None)
        )
    )
    .unwrap();
    writeln!(file, "{}", json!({"timestamp":"2030-01-01T00:30:00Z", "type":"event_msg", "payload":{"type":"token_count", "rate_limits":null}})).unwrap();
    writeln!(file, "not-json").unwrap();
    write!(file, "{{\"timestamp\":").unwrap();
    drop(file);
    modified(&path, "2030-01-01T02:00:00Z");
    let result = snapshot(read_limits(temp.path(), None));
    assert_eq!(result.session().unwrap().percent, 42.0);
    assert!(result.weekly().is_none());
    assert_eq!(result.windows.len(), 1);
}

#[test]
fn codex_rolled_window_uses_existing_reset_rendering() {
    // Known-bad: an expired percentage is printed as current headroom.
    let temp = TempDir::new().unwrap();
    session(
        temp.path(),
        1,
        None,
        &[event(
            "2030-01-01T01:00:00Z",
            "codex",
            window(300, json!(42), Some(at("2030-01-01T01:15:00Z").timestamp())),
            Value::Null,
        )],
        "2030-01-01T02:00:00Z",
    );
    let now = at("2030-01-01T01:30:00Z");
    let result = read_limits(temp.path(), None);
    let data = snapshot(result.clone());
    assert_eq!(crate::list_window(data.session(), now, false).0, "reset");
    assert!(
        format_info(&result, now, LimitSource::Codex)
            .contains("reset since the snapshot (was 42%)")
    );
}

#[test]
fn codex_limits_read_is_read_only() {
    // Known-bad: the reader creates a cache or modifies a Codex file.
    let temp = TempDir::new().unwrap();
    let path = session(
        temp.path(),
        1,
        None,
        &[event(
            "2030-01-01T01:00:00Z",
            "codex",
            window(300, json!(12), None),
            Value::Null,
        )],
        "2030-01-01T02:00:00Z",
    );
    fs::write(
        temp.path().join("auth.json"),
        br#"{"tokens":{"id_token":"h.e30.s"}}"#,
    )
    .unwrap();
    fs::write(temp.path().join("config.toml"), b"synthetic = true\n").unwrap();
    fs::create_dir_all(temp.path().join("archived_sessions")).unwrap();
    fs::write(
        temp.path().join("archived_sessions/rollout-2030-01-01T00-00-00-00000000-0000-4000-8000-000000000000.jsonl"),
        b"synthetic archived line\n",
    )
    .unwrap();
    fn state(root: &Path) -> Vec<FileState> {
        let mut pending = vec![root.to_path_buf()];
        let mut found = Vec::new();
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                let content = if metadata.is_file() {
                    Some((fs::read(&path).unwrap(), metadata.modified().unwrap()))
                } else {
                    pending.push(path.clone());
                    None
                };
                found.push((path.strip_prefix(root).unwrap().to_path_buf(), content));
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found
    }
    let before = state(temp.path());
    assert!(matches!(
        read_limits(temp.path(), None),
        Limits::Snapshot(_)
    ));
    assert_eq!(state(temp.path()), before);
    assert!(path.exists());
}
