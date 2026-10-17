use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const SESSION: &str = "00000000-0000-4000-8000-000000000001";

fn codex_session(home: &Path, response: &str, model: &str) {
    let dir = home.join("sessions/2030/01/01");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("rollout-2030-01-01-{response}-{SESSION}.jsonl"));
    let mut file = File::create(path).unwrap();
    writeln!(file, r#"{{"timestamp":"2030-01-01T00:00:00Z","type":"session_meta","payload":{{"id":"{SESSION}","source":"cli"}}}}"#).unwrap();
    writeln!(file, r#"{{"timestamp":"2030-01-01T00:00:00Z","type":"turn_context","payload":{{"turn_id":"turn-a","model":"{model}"}}}}"#).unwrap();
    writeln!(file, r#"{{"timestamp":"2030-01-01T01:00:00Z","type":"token_usage_record","payload":{{"response_id":"{response}","turn_id":"turn-a","usage":{{"input_tokens":100000,"output_tokens":10000}}}}}}"#).unwrap();
}

fn command(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cswitch"))
        .args(args)
        .env("HOME", home)
        .env("CODEX_HOME", home.join("other-codex-home"))
        .env_remove("CSWITCH_USAGE_DIR")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn output(home: &Path, args: &[&str]) -> String {
    let result = command(home, args);
    assert!(result.status.success(), "{}: {result:?}", args.join(" "));
    String::from_utf8(result.stdout).unwrap()
}

#[test]
fn built_binary_reports_codex_and_plain_home_then_prices_alias() {
    // Known-bad: a Codex session reaches the ledger but drops out before --by profile.
    let temp = TempDir::new().unwrap();
    let home = temp.path();
    let base = home.join(".claude-switch");
    let codex = base.join("profiles/spare");
    let claude = base.join("profiles/work");
    fs::create_dir_all(&claude).unwrap();
    fs::write(base.join("registry.json"),r#"{"profiles":{"spare":{"name":"spare","tool":"codex","email":"spare@example.com","added":"2030-01-01T00:00:00Z","last_used":null},"work":{"name":"work","tool":"claude","email":"work@example.com","added":"2030-01-01T00:00:00Z","last_used":null}}}"#).unwrap();
    codex_session(&codex, "response-spare", "codex-auto-review");
    codex_session(&home.join(".codex"), "response-default", "gpt-6-sol");
    let claude_file = claude.join("projects/demo/session.jsonl");
    fs::create_dir_all(claude_file.parent().unwrap()).unwrap();
    fs::write(&claude_file,r#"{"type":"assistant","timestamp":"2030-01-01T01:00:00Z","sessionId":"claude-session","requestId":"req-work","message":{"id":"msg-work","model":"claude-sonnet-5","usage":{"input_tokens":100000,"output_tokens":10000}}}
"#).unwrap();

    let before = [
        codex.join("sessions/2030/01/01"),
        home.join(".codex/sessions/2030/01/01"),
    ]
    .into_iter()
    .flat_map(|dir| fs::read_dir(dir).unwrap())
    .map(|entry| {
        let path = entry.unwrap().path();
        let bytes = fs::read(&path).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        (path, bytes, modified)
    })
    .collect::<Vec<(PathBuf, Vec<u8>, std::time::SystemTime)>>();

    let first = output(home, &["usage", "--since", "all", "--by", "profile"]);
    assert!(
        first.contains("codex:default") && first.contains("spare") && first.contains("work"),
        "{first}"
    );
    assert!(first.contains("$0.30") && first.contains('*'), "{first}");
    let selected = output(
        home,
        &[
            "usage",
            "--since",
            "all",
            "--profile",
            "codex:default",
            "--by",
            "profile",
        ],
    );
    assert!(
        selected.contains("codex:default") && !selected.contains("spare"),
        "{selected}"
    );
    output(home, &["usage", "alias", "codex-auto-review", "gpt-6-sol"]);
    let priced = output(home, &["usage", "--since", "all", "--by", "profile"]);
    assert!(
        priced.contains("spare") && !priced.contains('*'),
        "{priced}"
    );
    for (path, bytes, modified) in before {
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }
    output(home, &["remove", "spare", "--purge-usage"]);
    let after = output(home, &["usage", "--since", "all", "--by", "profile"]);
    assert!(!after.contains("spare"), "{after}");
    let cursors: serde_json::Value =
        serde_json::from_slice(&fs::read(base.join("usage/cursors.json")).unwrap()).unwrap();
    assert!(
        !cursors["files"]
            .as_object()
            .unwrap()
            .keys()
            .any(|path| path.contains("profiles/spare"))
    );
}
