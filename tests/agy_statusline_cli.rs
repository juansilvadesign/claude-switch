//! Built-binary check: the installed line runs inside an isolated Antigravity HOME.

use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn run(binary: &Path, home: &Path, args: &[&str], input: Option<&[u8]>) -> std::process::Output {
    let mut child = Command::new(binary)
        .args(args)
        .env("HOME", home)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(bytes) = input {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

#[test]
fn installed_antigravity_line_saves_offline_quota_from_unlinked_home() {
    // Known-bad: the command calls ProfileManager::new inside the fake HOME or saves under it.
    let temp = TempDir::new().unwrap();
    let real_home = temp.path().join("real");
    let base = real_home.join(".claude-switch");
    let profile_home = base.join("profiles/spare/home");
    let settings = profile_home.join(".gemini/antigravity-cli/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        br#"{"theme":"dark","statusLine":{"type":"","command":"","enabled":true}}"#,
    )
    .unwrap();
    fs::write(base.join("registry.json"), serde_json::to_vec(&json!({"profiles":{
        "spare":{"name":"spare","tool":"antigravity","email":"spare@example.com","added":"2030-01-01T00:00:00Z","last_used":null}
    }})).unwrap()).unwrap();
    let binary = Path::new(env!("CARGO_BIN_EXE_cswitch"));
    let installed = run(
        binary,
        &real_home,
        &["agy-statusline", "--install", "spare"],
        None,
    );
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let value: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    let command = value["statusLine"]["command"].as_str().unwrap();
    assert!(
        Path::new(
            command
                .split_whitespace()
                .next()
                .unwrap()
                .trim_matches('\'')
        )
        .is_absolute()
    );
    assert!(command.contains("--profile 'spare' --store '"));
    assert_eq!(value["statusLine"]["enabled"], true);
    let now = Utc::now();
    let reset_5h = (now + Duration::hours(3)).to_rfc3339();
    let reset_weekly = (now + Duration::days(5)).to_rfc3339();
    let input = json!({"terminal_width":80,"model":{"display_name":"Gemini 3.8 Flash"},"context_window":{"used_percentage":25},"plan_tier":"Google AI Pro","quota":{
        "gemini-5h":{"remaining_fraction":0.88,"reset_time":reset_5h},
        "gemini-weekly":{"remaining_fraction":0.65,"reset_time":reset_weekly},
        "3p-5h":{"remaining_fraction":0.99,"reset_time":reset_5h},
        "3p-weekly":{"remaining_fraction":0.98,"reset_time":reset_weekly}
    }});
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .env("HOME", &profile_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8(output.stdout).unwrap();
    assert!(line.starts_with("agy spare · 5h 12% · 7d 35%"), "{line}");
    assert!(!profile_home.join(".claude-switch").exists());
    let saved: Value =
        serde_json::from_slice(&fs::read(base.join("agy-quota/spare.json")).unwrap()).unwrap();
    assert_eq!(saved["quota"].as_object().unwrap().len(), 4);
    assert!(saved.get("email").is_none());
    let list = run(binary, &real_home, &["list"], None);
    assert!(list.status.success());
    let list = String::from_utf8(list.stdout).unwrap();
    let row = list
        .lines()
        .find(|line| line.starts_with("spare "))
        .unwrap();
    assert!(row.contains("12%") && row.contains("35%"), "{row}");
    let info = run(binary, &real_home, &["info", "spare"], None);
    assert!(info.status.success());
    let info = String::from_utf8(info.stdout).unwrap();
    assert!(
        info.contains("Google AI Pro") && info.contains("3p 5h"),
        "{info}"
    );
    let fake_bin = temp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_agy = fake_bin.join("agy");
    let refresh = json!({"command":{"data":{"groups":[
        {"buckets":[{"id":"gemini-5h","remaining_fraction":0.70,"reset_time":reset_5h},{"id":"gemini-weekly","remaining_fraction":0.60,"reset_time":reset_weekly}]},
        {"buckets":[{"id":"3p-5h","remaining_fraction":0.90,"reset_time":reset_5h},{"id":"3p-weekly","remaining_fraction":0.80,"reset_time":reset_weekly}]}
    ]}}});
    fs::write(
        &fake_agy,
        format!(
            "#!/bin/sh\nprintf '%s' '{}'\nprintf called > \"$HOME/agy-refresh-called\"\n",
            refresh
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_agy, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let offline = Command::new(binary)
        .args(["info", "spare"])
        .env("HOME", &real_home)
        .env("PATH", &fake_bin)
        .output()
        .unwrap();
    assert!(offline.status.success());
    assert!(!profile_home.join("agy-refresh-called").exists());
    let live = Command::new(binary)
        .args(["info", "spare", "--refresh-quota"])
        .env("HOME", &real_home)
        .env("PATH", &fake_bin)
        .output()
        .unwrap();
    assert!(
        live.status.success(),
        "{}",
        String::from_utf8_lossy(&live.stderr)
    );
    assert!(profile_home.join("agy-refresh-called").exists());
    assert!(String::from_utf8(live.stdout).unwrap().contains("30%"));
    let missing = run(
        binary,
        &profile_home,
        &[
            "agy-statusline",
            "--profile",
            "spare",
            "--store",
            base.to_str().unwrap(),
        ],
        Some(b"{}"),
    );
    assert!(missing.status.success());
    assert!(String::from_utf8(missing.stdout).unwrap().ends_with('\n'));
    let default = run(
        binary,
        &profile_home,
        &[
            "agy-statusline",
            "--profile",
            "antigravity:default",
            "--store",
            base.to_str().unwrap(),
        ],
        Some(&serde_json::to_vec(&input).unwrap()),
    );
    assert!(default.status.success());
    assert!(base.join("agy-quota/antigravity:default.json").exists());
}
