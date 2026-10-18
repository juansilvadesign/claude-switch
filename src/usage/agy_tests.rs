use super::agy::Reader;
use super::ledger::{Source, Store};
use super::metrics;
use super::rates::{cost, seed};
use super::report::{self, Options};
use crate::profile::{Profile, ProfileManager, Registry, Tool};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const CONVERSATION: &str = "00000000-0000-4000-8000-000000000001";
const OTHER: &str = "00000000-0000-4000-8000-000000000002";
const AT: &str = "2030-01-01T12:00:00Z";

fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(AT)
        .unwrap()
        .with_timezone(&Utc)
}

fn step(source: &str, index: u64, input: u64, output: u64, cache: u64) -> String {
    json!({"source":source,"type":"PLANNER_RESPONSE","status":"DONE",
        "step_index":index,"created_at":AT,"input_tokens":input,
        "output_tokens":output,"cache_read_tokens":cache,
        "content":"CANARY_SYNTHETIC_PROMPT_0000"})
    .to_string()
}

fn file(gemini: &Path, variant: &str, conversation: &str, name: &str) -> PathBuf {
    gemini
        .join(variant)
        .join("brain")
        .join(conversation)
        .join(".system_generated/logs")
        .join(name)
}

fn write_lines(path: &Path, lines: &[String]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::File::create(path).unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
}

fn append_lines(path: &Path, lines: &[String]) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
}

struct Fixture {
    _temp: TempDir,
    gemini: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let gemini = temp.path().join(".gemini");
        let mut store = Store::new(temp.path().join("ledger"), Vec::new());
        store.agy_sources.push(Source {
            profile: "antigravity:default".into(),
            directory: gemini.clone(),
        });
        Self {
            _temp: temp,
            gemini,
            store,
        }
    }

    fn transcript(&self, variant: &str, conversation: &str, name: &str) -> PathBuf {
        file(&self.gemini, variant, conversation, name)
    }
}

#[test]
fn agy_model_steps_become_rows_and_non_model_steps_do_not() {
    // Known-bad: USER_EXPLICIT or SYSTEM rows are counted as model usage.
    let reader = Reader::new(CONVERSATION);
    let model = reader
        .line(step("MODEL", 1, 11, 7, 13).as_bytes(), "sample")
        .unwrap()
        .unwrap();
    assert_eq!((model.input, model.output, model.cache_read), (11, 7, 13));
    assert_eq!(model.session, CONVERSATION);
    assert_eq!(model.time, at());
    assert_eq!(
        model.key,
        json!(["antigravity", CONVERSATION, 1]).to_string()
    );
    for source in ["USER_EXPLICIT", "SYSTEM"] {
        assert!(
            reader
                .line(step(source, 2, 19, 23, 29).as_bytes(), "sample")
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn agy_input_excludes_cache_read_tokens() {
    // Known-bad: cache_read_tokens are added to input_tokens as in Codex.
    let request = Reader::new(CONVERSATION)
        .line(step("MODEL", 1, 5, 3, 41).as_bytes(), "sample")
        .unwrap()
        .unwrap();
    assert_eq!(request.input, 5);
    assert_eq!(request.cache_read, 41);
    assert_eq!(request.cache_write_5m + request.cache_write_1h, 0);
}

#[test]
fn agy_growing_transcript_cursor_does_not_replay_steps() {
    // Known-bad: resuming a growing file replays steps before the cursor.
    let fixture = Fixture::new();
    let path = fixture.transcript("antigravity-cli", CONVERSATION, "transcript_full.jsonl");
    write_lines(&path, &[step("MODEL", 1, 5, 3, 41)]);
    assert_eq!(fixture.store.ingest().unwrap().new_requests, 1);
    append_lines(&path, &[step("MODEL", 2, 7, 11, 13)]);
    let second = fixture.store.ingest().unwrap();
    assert_eq!((second.new_requests, second.duplicates), (1, 0));
    assert_eq!(fixture.store.ingest().unwrap().new_requests, 0);
    let ledger = fixture.store.load().unwrap();
    assert_eq!(ledger.requests.len(), 2);
    assert_eq!(ledger.cursors.duplicates, 0);
    assert_eq!(ledger.sessions[0].id, CONVERSATION);
}

#[test]
fn agy_prefers_one_full_transcript_over_short_transcript() {
    // Known-bad: summing transcript.jsonl and transcript_full.jsonl doubles each step.
    let fixture = Fixture::new();
    write_lines(
        &fixture.transcript("antigravity-cli", CONVERSATION, "transcript.jsonl"),
        &[step("MODEL", 1, 101, 3, 5)],
    );
    write_lines(
        &fixture.transcript("antigravity-cli", CONVERSATION, "transcript_full.jsonl"),
        &[step("MODEL", 1, 7, 11, 13)],
    );
    let ingest = fixture.store.ingest().unwrap();
    let ledger = fixture.store.load().unwrap();
    assert_eq!((ingest.new_requests, ingest.duplicates), (1, 0));
    assert_eq!(
        (ledger.requests[0].input, ledger.requests[0].output),
        (7, 11)
    );
    assert_eq!(ledger.cursors.files.len(), 1);
}

#[test]
fn agy_reads_both_brain_roots_and_collapses_shared_conversation() {
    // Known-bads: only one version's root is read, or the same conversation is counted twice.
    let fixture = Fixture::new();
    for (variant, conversation, index) in [
        ("antigravity-cli", CONVERSATION, 1),
        ("antigravity", CONVERSATION, 1),
        ("antigravity", OTHER, 2),
    ] {
        write_lines(
            &fixture.transcript(variant, conversation, "transcript.jsonl"),
            &[step("MODEL", index, 7, 11, 13)],
        );
    }
    let ingest = fixture.store.ingest().unwrap();
    let ledger = fixture.store.load().unwrap();
    assert_eq!((ingest.new_requests, ingest.duplicates), (2, 1));
    assert_eq!(ledger.requests.len(), 2);
    assert_eq!(ledger.sessions.len(), 2);
}

#[test]
fn agy_rows_have_no_model_or_dollar_value_even_with_an_alias() {
    // Known-bad: an unknown-model rate lookup yields $*, or an alias prices Antigravity.
    let fixture = Fixture::new();
    write_lines(
        &fixture.transcript("antigravity", CONVERSATION, "transcript.jsonl"),
        &[step("MODEL", 1, 5, 3, 41)],
    );
    fs::create_dir_all(&fixture.store.dir).unwrap();
    let mut rates = seed();
    rates.aliases.insert(String::new(), "gpt-6-sol".into());
    fs::write(
        fixture.store.dir.join("rates.json"),
        serde_json::to_vec_pretty(&rates).unwrap(),
    )
    .unwrap();
    let now = DateTime::parse_from_rfc3339("2030-01-02T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let output = report::run(
        &fixture.store,
        &Options {
            since: "all".into(),
            profile: Some("antigravity:default".into()),
            by: Some("model".into()),
            json: false,
            explain: None,
            unattributed: false,
        },
        now,
    )
    .unwrap();
    assert!(output.contains("—"));
    assert!(!output.contains("$*"));
    assert!(!output.contains("$0.00"));
    let row = &fixture.store.load().unwrap().requests[0];
    assert!(row.model.is_empty());
    assert!(cost(row, &rates).is_none());
    let request_json: Value = serde_json::from_slice(
        &fs::read(fixture.store.dir.join("requests/2030-01.jsonl")).unwrap(),
    )
    .unwrap();
    assert!(request_json.get("model").is_none());
    let hourly = metrics::read(&fixture.store.dir).unwrap();
    let hourly_json: Value =
        serde_json::from_slice(&fs::read(fixture.store.dir.join("hourly.json")).unwrap()).unwrap();
    let chats_json: Value =
        serde_json::from_slice(&fs::read(fixture.store.dir.join("chats.json")).unwrap()).unwrap();
    assert!(hourly_json["rows"][0].get("model").is_none());
    assert!(chats_json["rows"][0].get("model").is_none());
    let amounts = metrics::windows(
        &hourly.rows,
        "antigravity:default",
        now,
        chrono::FixedOffset::east_opt(0).unwrap(),
        &rates,
        None,
        false,
    );
    assert_eq!(amounts[2].tokens, 49);
    assert_eq!(amounts[2].list_cell(), "—");
    assert!(
        !fs::read_to_string(fixture.store.dir.join("summary.json"))
            .unwrap()
            .contains("cost_usd")
    );
    let json_output = report::run(
        &fixture.store,
        &Options {
            since: "all".into(),
            profile: Some("antigravity:default".into()),
            by: None,
            json: true,
            explain: None,
            unattributed: false,
        },
        now,
    )
    .unwrap();
    let json: Value = serde_json::from_str(&json_output).unwrap();
    assert!(json["rows"][0].get("model").is_none());
    assert!(json["rows"][0]["cost_usd"].is_null());
}

#[test]
fn agy_plain_home_uses_distinct_default_account_key() {
    // Known-bad: plain .gemini sessions are folded into a profile or agy-<name> alias.
    let temp = TempDir::new().unwrap();
    let manager = ProfileManager::with_base_dir(temp.path().join(".claude-switch")).unwrap();
    let mut registry = Registry::default();
    registry.profiles.insert(
        "spare".into(),
        Profile {
            name: "spare".into(),
            tool: Tool::Antigravity,
            email: Some("spare@example.com".into()),
            added: at(),
            last_used: None,
        },
    );
    fs::write(
        manager.base_dir.join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    let store = super::store(&manager, None).unwrap();
    let default = store
        .agy_sources
        .iter()
        .find(|source| source.profile == "antigravity:default")
        .unwrap();
    let profile = store
        .agy_sources
        .iter()
        .find(|source| source.profile == "spare")
        .unwrap();
    assert_ne!(default.directory, profile.directory);
    assert_ne!(default.profile, "spare");
    assert_ne!(default.profile, "agy-spare");
    write_lines(
        &file(
            &default.directory,
            "antigravity",
            CONVERSATION,
            "transcript.jsonl",
        ),
        &[step("MODEL", 1, 5, 3, 7)],
    );
    write_lines(
        &file(&profile.directory, "antigravity", OTHER, "transcript.jsonl"),
        &[step("MODEL", 1, 11, 13, 17)],
    );
    assert_eq!(store.ingest().unwrap().new_requests, 2);
    let rows = store.load().unwrap().requests;
    assert!(rows.iter().any(|row| row.profile == "antigravity:default"));
    assert!(rows.iter().any(|row| row.profile == "spare"));
}

#[test]
fn agy_malformed_line_does_not_end_transcript_read() {
    // Known-bad: one invalid JSON or token-bearing MODEL step aborts the read.
    let fixture = Fixture::new();
    write_lines(
        &fixture.transcript("antigravity-cli", CONVERSATION, "transcript_full.jsonl"),
        &[
            step("MODEL", 1, 5, 3, 7),
            "{bad-json".into(),
            json!({"source":"MODEL","step_index":2,"created_at":AT,
                "input_tokens":"bad","output_tokens":3,"cache_read_tokens":7})
            .to_string(),
            step("MODEL", 3, 11, 13, 17),
        ],
    );
    let ingest = fixture.store.ingest().unwrap();
    assert_eq!((ingest.new_requests, ingest.malformed), (2, 2));
    assert_eq!(fixture.store.load().unwrap().requests.len(), 2);
}
