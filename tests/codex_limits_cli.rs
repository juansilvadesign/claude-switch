use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

fn session(home: &Path, percent: u8) {
    let dir = home.join("sessions/2030/01/01");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout-2030-01-01T00-00-00-00000000-0000-4000-8000-000000000000.jsonl");
    let mut file = File::create(&path).unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"2030-01-01T00:00:00Z","type":"session_meta","payload":{{"id":"00000000-0000-4000-8000-000000000000"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"2030-01-01T01:00:00Z","type":"event_msg","payload":{{"type":"token_count","info":null,"rate_limits":{{"limit_id":"codex","primary":{{"used_percent":{percent},"window_minutes":300,"resets_at":1893546000}},"secondary":{{"used_percent":85,"window_minutes":10080,"resets_at":1894064400}}}}}}}}"#
    )
    .unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(1_893_549_600))
        .unwrap();
}

fn command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cswitch"));
    command
        .env("HOME", home)
        .env("CODEX_HOME", home.join("other-codex-home"))
        .env_remove("CSWITCH_USAGE_DIR")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("NO_COLOR", "1")
        .env("COLUMNS", "200");
    command
}

fn files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

#[test]
fn built_binary_reads_codex_limits_through_list_info_and_statusline() {
    // Known-bad: a CLI route bypasses the Codex reader and prints no report or hint.
    let home = TempDir::new().unwrap();
    let root = home.path();
    let base = root.join(".claude-switch");
    let claude = base.join("profiles/work");
    let codex = base.join("profiles/spare-gpt");
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        base.join("registry.json"),
        r#"{"profiles":{"work":{"name":"work","tool":"claude","email":"work@example.com","added":"2030-01-01T00:00:00Z","last_used":null},"spare-gpt":{"name":"spare-gpt","tool":"codex","email":"spare-gpt@example.com","added":"2030-01-01T00:00:00Z","last_used":null}}}"#,
    )
    .unwrap();
    session(&codex, 12);
    session(&root.join(".codex"), 50);
    let before = files(root);

    let list = command(root).arg("list").output().unwrap();
    assert!(list.status.success(), "{list:?}");
    let text = String::from_utf8(list.stdout).unwrap();
    let row = text
        .lines()
        .find(|line| line.starts_with("spare-gpt "))
        .unwrap();
    assert!(
        row.contains("12%") && row.contains("85% █████████░"),
        "{row}"
    );
    assert!(row.contains("—"), "{row}");
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("default "))
            .count(),
        0
    );

    let info = command(root).args(["info", "spare-gpt"]).output().unwrap();
    assert!(info.status.success(), "{info:?}");
    let text = String::from_utf8(info.stdout).unwrap();
    assert!(text.contains("Plan limits (Codex's last report,"), "{text}");
    assert!(text.contains("Session (5h)   12%"), "{text}");
    assert!(text.contains("Weekly (7d)    85%"), "{text}");

    let mut status = command(root);
    status
        .args(["statusline", "--no-refresh"])
        .env("CLAUDE_CONFIG_DIR", &claude)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = status.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"rate_limits":{"five_hour":{"used_percentage":90}}}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let line = String::from_utf8(output.stdout).unwrap();
    assert!(line.contains("→ codex spare-gpt 5h 12%"), "{line}");
    assert_eq!(files(root), before);

    session(&codex, 100);
    let before = files(root);
    let mut status = command(root);
    status
        .args(["statusline", "--no-refresh"])
        .env("CLAUDE_CONFIG_DIR", &claude)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = status.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"rate_limits":{"five_hour":{"used_percentage":90}}}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let line = String::from_utf8(output.stdout).unwrap();
    assert!(line.contains("→ codex default 5h 50%"), "{line}");
    assert_eq!(files(root), before);
}
