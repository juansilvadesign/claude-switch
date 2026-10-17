//! Antigravity status line and explicit live quota refresh.

use crate::agy;
use crate::agy_limits;
use crate::atomic;
use crate::key;
use crate::profile::{self, ProfileManager, Tool};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub fn command_line(base: &Path, account: &str, now: DateTime<Utc>) -> String {
    let input = if io::stdin().is_terminal() {
        Value::Null
    } else {
        let mut bytes = Vec::new();
        let _ = io::stdin().take(1_048_576).read_to_end(&mut bytes);
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    render_and_save(&input, base, account, now)
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn render_and_save(input: &Value, base: &Path, account: &str, now: DateTime<Utc>) -> String {
    let label = if account == agy_limits::DEFAULT_ACCOUNT {
        "default"
    } else {
        account
    };
    let mut parts = vec![format!("agy {}", clean(label))];
    if let Some(quota) = input.get("quota")
        && let Some(snapshot) = agy_limits::snapshot(quota, now)
    {
        let _ = agy_limits::save(
            base,
            account,
            quota,
            input.get("plan_tier").and_then(Value::as_str),
            now,
        );
        if let Some(window) = snapshot.session() {
            parts.push(format!("5h {}", window.percent_label(now)));
        }
        if let Some(window) = snapshot.weekly() {
            parts.push(format!("7d {}", window.percent_label(now)));
        }
    }
    if let Some(used) = input
        .get("context_window")
        .and_then(|v| v.get("used_percentage"))
        .and_then(Value::as_f64)
        && used.is_finite()
    {
        parts.push(format!("ctx {:.0}%", used.clamp(0.0, 100.0).round()));
    }
    for text in [
        input
            .get("model")
            .and_then(|v| v.get("display_name"))
            .and_then(Value::as_str),
        input.get("plan_tier").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    {
        let text = clean(text);
        if !text.is_empty() {
            parts.push(text);
        }
    }
    let width = input
        .get("terminal_width")
        .and_then(Value::as_u64)
        .unwrap_or(80)
        .min(500) as usize;
    let mut output = String::new();
    for part in parts {
        let next = if output.is_empty() {
            part.clone()
        } else {
            format!("{output} · {part}")
        };
        if next.chars().count() <= width {
            output = next;
        } else if output.is_empty() {
            output = part.chars().take(width).collect();
            break;
        }
    }
    output
}

pub fn manage(
    manager: &ProfileManager,
    name: &str,
    install: bool,
    force: bool,
    executable: &Path,
    now: DateTime<Utc>,
) -> Result<bool> {
    let executable = fs::canonicalize(executable).context("Cannot resolve cswitch executable")?;
    let base = fs::canonicalize(&manager.base_dir).context("Cannot resolve profile store")?;
    let command = install.then(|| {
        format!(
            "{} agy-statusline --profile {} --store {}",
            profile::shell_quote(&executable.to_string_lossy()),
            profile::shell_quote(name),
            profile::shell_quote(&base.to_string_lossy())
        )
    });
    key::edit_agy_statusline(manager, name, command.as_deref(), force, &executable, now)
}

/// The only path that starts `agy`: called by an explicit CLI flag or TUI key.
pub fn refresh_for_profile(manager: &ProfileManager, name: &str, now: DateTime<Utc>) -> Result<()> {
    if manager.get_profile(name)?.tool != Tool::Antigravity {
        bail!("Quota refresh requires an Antigravity profile");
    }
    let home = agy::profile_home(&manager.profile_dir(name));
    refresh_with(
        Path::new(agy::AGY_PROGRAM),
        &home,
        &manager.base_dir,
        name,
        now,
        Duration::from_secs(8),
    )
}

pub fn refresh_with(
    program: &Path,
    home: &Path,
    base: &Path,
    account: &str,
    now: DateTime<Utc>,
    timeout: Duration,
) -> Result<()> {
    let (temporary, output) = atomic::create_private_temp(&base.join("agy-quota"))?;
    let result = (|| -> Result<()> {
        let mut child = Command::new(program)
            .args(["-p", "/quota", "--output-format", "json"])
            .env("HOME", home)
            .stdin(Stdio::null())
            .stdout(Stdio::from(output))
            .stderr(Stdio::null())
            .spawn()
            .context("Could not start Antigravity quota refresh")?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Antigravity quota refresh timed out; saved quota kept");
            }
            thread::sleep(Duration::from_millis(50));
        };
        if !status.success() {
            bail!("Antigravity quota refresh failed; saved quota kept");
        }
        if fs::metadata(&temporary)?.len() > 1_048_576 {
            bail!("Antigravity quota response was too large; saved quota kept");
        }
        let bytes = fs::read(&temporary)?;
        let value: Value =
            serde_json::from_slice(&bytes).context("Invalid quota response; saved quota kept")?;
        let quota = agy_limits::quota_from_refresh(&value)
            .context("Incomplete quota response; saved quota kept")?;
        let tier = value
            .get("plan_tier")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| agy_limits::plan_tier(base, account, now));
        if !agy_limits::save(base, account, &quota, tier.as_deref(), now)? {
            bail!("Incomplete quota response; saved quota kept");
        }
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use chrono::TimeZone;
    use serde_json::json;
    use tempfile::TempDir;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2030, 1, 1, 12, 0, 0).unwrap()
    }

    fn input() -> Value {
        json!({"terminal_width":80,"model":{"display_name":"Gemini 3.8 Flash"},"context_window":{"used_percentage":25},"plan_tier":"Google AI Pro","quota":{
            "gemini-5h":{"remaining_fraction":0.88,"reset_time":"2030-01-01T15:00:00Z"},
            "gemini-weekly":{"remaining_fraction":0.65,"reset_time":"2030-01-07T12:00:00Z"},
            "3p-5h":{"remaining_fraction":0.99,"reset_time":"2030-01-01T16:00:00Z"},
            "3p-weekly":{"remaining_fraction":0.98,"reset_time":"2030-01-08T12:00:00Z"}
        }})
    }

    #[test]
    fn line_saves_quota_under_explicit_account_and_obeys_width() {
        // Known-bad: render without saving, use HOME's store, or exceed terminal_width.
        let temp = TempDir::new().unwrap();
        let mut input = input();
        input["terminal_width"] = json!(32);
        let line = render_and_save(&input, temp.path(), "spare", now());
        assert!(line.chars().count() <= 32, "{line}");
        assert!(line.starts_with("agy spare · 5h 12%"), "{line}");
        assert!(agy_limits::path(temp.path(), "spare").unwrap().exists());
        assert!(
            !agy_limits::path(temp.path(), agy_limits::DEFAULT_ACCOUNT)
                .unwrap()
                .exists()
        );
    }

    #[test]
    fn missing_fields_render_without_panic_or_save() {
        // Known-bad: missing model or quota panics or overwrites the saved figure.
        let temp = TempDir::new().unwrap();
        let line = render_and_save(
            &json!({"terminal_width":8}),
            temp.path(),
            agy_limits::DEFAULT_ACCOUNT,
            now(),
        );
        assert!(line.chars().count() <= 8);
        let narrow = render_and_save(&json!({"terminal_width":5}), temp.path(), "spare", now());
        assert!(narrow.chars().count() <= 5);
        assert!(
            !agy_limits::path(temp.path(), agy_limits::DEFAULT_ACCOUNT)
                .unwrap()
                .exists()
        );
    }

    #[cfg(unix)]
    #[test]
    fn live_refresh_is_explicit_and_failure_keeps_saved_quota() {
        // Known-bad: spawning agy on an offline render, or deleting the old figure on failure.
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new().unwrap();
        let base = temp.path().join("store");
        let home = temp.path().join("profile-home");
        fs::create_dir_all(&home).unwrap();
        let old = input();
        assert!(
            agy_limits::save(&base, "spare", &old["quota"], Some("Google AI Pro"), now()).unwrap()
        );
        let before = fs::read(agy_limits::path(&base, "spare").unwrap()).unwrap();
        let program = temp.path().join("agy");
        let refresh = json!({"command":{"data":{"groups":[
            {"buckets":[{"id":"gemini-5h","remaining_fraction":0.70,"reset_time":"2030-01-01T17:00:00Z"},{"id":"gemini-weekly","remaining_fraction":0.60,"reset_time":"2030-01-07T12:00:00Z"}]},
            {"buckets":[{"id":"3p-5h","remaining_fraction":0.90,"reset_time":"2030-01-01T17:00:00Z"},{"id":"3p-weekly","remaining_fraction":0.80,"reset_time":"2030-01-08T12:00:00Z"}]}
        ]}}});
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nprintf '%s' '{}'\nprintf called > \"$HOME/agy-called\"\n",
                refresh
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            fs::read(agy_limits::path(&base, "spare").unwrap()).unwrap(),
            before
        );
        assert!(!home.join("agy-called").exists());
        let _ = render_and_save(&json!({"terminal_width":20}), &base, "spare", now());
        assert!(!home.join("agy-called").exists());
        refresh_with(
            &program,
            &home,
            &base,
            "spare",
            now() + chrono::Duration::minutes(1),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(home.join("agy-called").exists());
        let Limits::Snapshot(snapshot) =
            agy_limits::read_limits(&base, "spare", now() + chrono::Duration::minutes(1))
        else {
            panic!("expected refreshed quota")
        };
        assert!((snapshot.session().unwrap().percent - 30.0).abs() < 0.001);
        let refreshed = fs::read(agy_limits::path(&base, "spare").unwrap()).unwrap();
        assert_ne!(refreshed, before);
        fs::write(&program, b"#!/bin/sh\nexit 2\n").unwrap();
        assert!(
            refresh_with(
                &program,
                &home,
                &base,
                "spare",
                now() + chrono::Duration::minutes(2),
                Duration::from_secs(1)
            )
            .is_err()
        );
        assert_eq!(
            fs::read(agy_limits::path(&base, "spare").unwrap()).unwrap(),
            refreshed
        );
        fs::write(&program, b"#!/bin/sh\nsleep 1\n").unwrap();
        let error = refresh_with(
            &program,
            &home,
            &base,
            "spare",
            now() + chrono::Duration::minutes(3),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert_eq!(
            fs::read(agy_limits::path(&base, "spare").unwrap()).unwrap(),
            refreshed
        );
    }
}
