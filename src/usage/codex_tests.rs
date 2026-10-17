use super::codex::Reader;
use super::ledger::{Source, Store};
use super::rates::{cost, load_or_seed, seed};
use crate::profile::{Profile, ProfileManager, Registry, Tool};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

const THREAD: &str = "00000000-0000-4000-8000-000000000001";
const ROOT: &str = "00000000-0000-4000-8000-000000000002";
const AT: &str = "2030-01-01T12:00:00Z";

fn at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .unwrap()
        .with_timezone(&Utc)
}

fn row(kind: &str, at: &str, payload: Value) -> String {
    format!(r#"{{"timestamp":"{at}","type":"{kind}","payload":{payload}}}"#)
}

fn meta(id: &str, root: Option<&str>, source: Value) -> String {
    row(
        "session_meta",
        AT,
        json!({"id":id,"session_id":root,"source":source,"cli_version":"0.200.0","cwd":"/srv/example/demo"}),
    )
}

fn turn(id: &str, model: &str) -> String {
    row(
        "turn_context",
        AT,
        json!({"turn_id":id,"model":model,"cwd":"/srv/example/demo"}),
    )
}

fn usage(input: u64, cached: u64, written: u64, output: u64) -> Value {
    json!({"input_tokens":input,"cached_input_tokens":cached,
        "cache_write_input_tokens":written,"output_tokens":output,
        "reasoning_output_tokens":output / 2,"total_tokens":input + output})
}

fn record(id: &str, turn_id: &str, at: &str, usage: Value) -> String {
    row(
        "token_usage_record",
        at,
        json!({"response_id":id,"turn_id":turn_id,"usage":usage,
            "turn_token_usage":{"input_tokens":999999},
            "thread_token_usage":{"input_tokens":999999}}),
    )
}

fn count(at: &str, total: u64, last: Value) -> String {
    format!(
        r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total}}},"last_token_usage":{last}}}}}}}"#
    )
}

fn settings(tier: &str) -> String {
    format!(
        r#"{{"timestamp":"{AT}","type":"event_msg","payload":{{"type":"thread_settings_applied","thread_settings":{{"service_tier":"{tier}"}}}}}}"#
    )
}

fn write_lines(path: &Path, lines: &[String]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = File::create(path).unwrap();
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
    temp: TempDir,
    home: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("codex-home");
        let mut store = Store::new(temp.path().join("ledger"), Vec::new());
        store.codex_sources.push(Source {
            profile: "codex:default".into(),
            directory: home.clone(),
        });
        Self { temp, home, store }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.home.join(format!("sessions/2030/01/01/{name}.jsonl"))
    }
}

#[test]
fn codex_record_uses_own_counts_and_prices_once() {
    // Known-bads: cached or written input remains in input, reasoning is added to output,
    // or turn/thread totals become a request.
    let mut reader = Reader::default();
    reader
        .line(meta(THREAD, None, json!("cli")).as_bytes(), "sample")
        .unwrap();
    reader
        .line(turn("turn-a", "gpt-6-sol").as_bytes(), "sample")
        .unwrap();
    let mut counts = usage(200_000, 120_000, 20_000, 10_000);
    counts["reasoning_output_tokens"] = json!(8_000);
    let request = reader
        .line(
            record("response-a", "turn-a", AT, counts).as_bytes(),
            "sample",
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            request.input,
            request.cache_read,
            request.cache_write_5m,
            request.output
        ),
        (60_000, 120_000, 20_000, 10_000)
    );
    assert_eq!(request.cache_write_1h, 0);
    assert_eq!(request.model, "gpt-6-sol");
    assert!((cost(&request, &seed()).unwrap().total() - 0.294).abs() < 1e-9);
}

#[test]
fn codex_two_ingests_equal_one_without_replaying_prior_requests() {
    // Known-bads: context begins at the cursor, or old requests are sent again.
    let lines = vec![
        meta(THREAD, None, json!("cli")),
        turn("turn-a", "gpt-6-sol"),
        record("response-a", "turn-a", AT, usage(100, 10, 0, 20)),
        turn("turn-b", "gpt-6-astra"),
        record(
            "response-b",
            "turn-b",
            "2030-01-01T12:01:00Z",
            usage(200, 20, 0, 30),
        ),
    ];
    let whole = Fixture::new();
    write_lines(&whole.file("one"), &lines);
    whole.store.ingest().unwrap();
    let split = Fixture::new();
    write_lines(&split.file("one"), &lines[..3]);
    split.store.ingest().unwrap();
    append_lines(&split.file("one"), &lines[3..]);
    let second = split.store.ingest().unwrap();
    assert_eq!(second.new_requests, 1);
    assert_eq!(second.duplicates, 0);
    let rows = |store: &Store| {
        let mut rows = store
            .load()
            .unwrap()
            .requests
            .into_iter()
            .map(|row| (row.key, row.model, row.input))
            .collect::<Vec<_>>();
        rows.sort();
        rows
    };
    assert_eq!(rows(&whole.store), rows(&split.store));
    assert_eq!(split.store.load().unwrap().cursors.duplicates, 0);
}

#[test]
fn codex_partial_record_waits_and_bad_request_lines_are_counted() {
    // Known-bad: a cut line is marked malformed or its later completion is skipped.
    let fixture = Fixture::new();
    let path = fixture.file("partial");
    write_lines(
        &path,
        &[
            meta(THREAD, None, json!("cli")),
            turn("turn-a", "gpt-6-sol"),
        ],
    );
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    write!(
        file,
        "{}",
        &record("response-a", "turn-a", AT, usage(100, 0, 0, 10))[..80]
    )
    .unwrap();
    drop(file);
    let first = fixture.store.ingest().unwrap();
    assert_eq!(
        (first.new_requests, first.partial, first.malformed),
        (0, 1, 0)
    );
    let record = record("response-a", "turn-a", AT, usage(100, 0, 0, 10));
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file, "{}", &record[80..]).unwrap();
    writeln!(file, r#"{{"timestamp":"bad","type":"token_usage_record","payload":{{"response_id":"response-b","usage":{{}}}}}}"#).unwrap();
    writeln!(file, "{{\"type\":\"token_usage_record\",not-json").unwrap();
    drop(file);
    let second = fixture.store.ingest().unwrap();
    assert_eq!(
        (second.new_requests, second.partial, second.malformed),
        (1, 0, 2)
    );
    assert_eq!(fixture.store.load().unwrap().requests.len(), 1);
}

#[test]
fn codex_model_uses_only_earlier_turn_context() {
    // Known-bad: a later matching turn supplies a model to an earlier request.
    let mut reader = Reader::default();
    reader
        .line(meta(THREAD, None, json!("cli")).as_bytes(), "sample")
        .unwrap();
    reader
        .line(turn("turn-a", "gpt-6-sol").as_bytes(), "sample")
        .unwrap();
    let matched = reader
        .line(
            record("response-a", "turn-a", AT, usage(1, 0, 0, 1)).as_bytes(),
            "sample",
        )
        .unwrap()
        .unwrap();
    let fallback = reader
        .line(
            record(
                "response-b",
                "turn-b",
                "2030-01-01T12:00:00.100Z",
                usage(1, 0, 0, 1),
            )
            .as_bytes(),
            "sample",
        )
        .unwrap()
        .unwrap();
    reader
        .line(turn("turn-b", "gpt-6-astra").as_bytes(), "sample")
        .unwrap();
    assert_eq!(matched.model, "gpt-6-sol");
    assert_eq!(fallback.model, "gpt-6-sol");
    let mut no_turn = Reader::default();
    no_turn
        .line(meta(ROOT, None, json!("cli")).as_bytes(), "sample")
        .unwrap();
    let unknown = no_turn
        .line(
            record("response-c", "missing", AT, usage(1, 0, 0, 1)).as_bytes(),
            "sample",
        )
        .unwrap()
        .unwrap();
    assert_eq!(unknown.model, "(unknown)");
    assert!(cost(&unknown, &seed()).is_none());
}

#[test]
fn codex_response_id_deduplicates_session_and_archive() {
    // Known-bad: a response key includes the source file's path.
    let fixture = Fixture::new();
    let lines = [
        meta(THREAD, None, json!("cli")),
        turn("turn-a", "gpt-6-sol"),
        record("response-a", "turn-a", AT, usage(100, 0, 0, 20)),
    ];
    write_lines(&fixture.file("one"), &lines);
    write_lines(
        &fixture
            .home
            .join("archived_sessions/rollout-2030-01-01.jsonl"),
        &lines,
    );
    let result = fixture.store.ingest().unwrap();
    assert_eq!((result.new_requests, result.duplicates), (1, 1));
    assert_eq!(fixture.store.load().unwrap().requests.len(), 1);
}

#[test]
fn codex_fallback_counts_changed_totals_only_until_first_record() {
    // Known-bads: repeated or null totals count, post-record counts count,
    // or a copied event gets a path-specific key.
    let fixture = Fixture::new();
    let first = count(AT, 120, usage(100, 20, 0, 20));
    let repeated = count("2030-01-01T12:00:01Z", 120, usage(100, 20, 0, 20));
    let second = count("2030-01-01T12:01:00Z", 240, usage(100, 20, 0, 20));
    let null = format!(
        r#"{{"timestamp":"{AT}","type":"event_msg","payload":{{"type":"token_count","info":null}}}}"#
    );
    let lines = [
        meta(THREAD, None, json!("cli")),
        turn("turn-a", "gpt-6-sol"),
        first.clone(),
        repeated,
        null,
        second,
        record(
            "response-a",
            "turn-a",
            "2030-01-01T12:02:00Z",
            usage(100, 0, 0, 20),
        ),
        count("2030-01-01T12:03:00Z", 360, usage(100, 0, 0, 20)),
    ];
    write_lines(&fixture.file("one"), &lines);
    write_lines(
        &fixture.file("two"),
        &[
            meta(THREAD, None, json!("cli")),
            turn("turn-a", "gpt-6-sol"),
            first,
        ],
    );
    let result = fixture.store.ingest().unwrap();
    assert_eq!((result.new_requests, result.duplicates), (3, 1));
    let rows = fixture.store.load().unwrap().requests;
    assert_eq!(
        rows.iter()
            .filter(|row| row.key.contains("codex-count"))
            .count(),
        2
    );
    assert_eq!(rows[0].input, 80);
}

#[test]
fn codex_session_uses_root_and_first_meta_only() {
    // Known-bad: the last session_meta replaces the file's own root and sidechain state.
    let fixture = Fixture::new();
    write_lines(
        &fixture.file("agent"),
        &[
            meta(THREAD, Some(ROOT), json!({"subagent":"review"})),
            meta("00000000-0000-4000-8000-000000000003", None, json!("cli")),
            turn("turn-a", "gpt-6-sol"),
            record("response-a", "turn-a", AT, usage(1, 0, 0, 1)),
        ],
    );
    write_lines(
        &fixture.file("old"),
        &[
            meta(THREAD, None, json!("cli")),
            record("response-b", "missing", AT, usage(1, 0, 0, 1)),
        ],
    );
    fixture.store.ingest().unwrap();
    let rows = fixture.store.load().unwrap().requests;
    assert_eq!(rows[0].session, ROOT);
    assert!(rows[0].sidechain);
    assert_eq!(rows[1].session, THREAD);
    assert!(!rows[1].sidechain);
}

#[test]
fn codex_sources_are_separate_from_claude_and_purge_cursors() {
    // Known-bads: CODEX_HOME overrides the plain home, or Codex projects use Claude parsing.
    let temp = TempDir::new().unwrap();
    let manager =
        ProfileManager::with_paths(temp.path().join("switch"), temp.path().join(".claude"))
            .unwrap();
    let mut registry = Registry::default();
    for (name, tool) in [("spare", Tool::Codex), ("old", Tool::Antigravity)] {
        registry.profiles.insert(
            name.into(),
            Profile {
                name: name.into(),
                tool,
                email: Some(format!("{name}@example.com")),
                added: at(AT),
                last_used: None,
            },
        );
        fs::create_dir_all(manager.profile_dir(name)).unwrap();
    }
    fs::write(
        manager.base_dir.join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    let plain = temp.path().join(".codex");
    let profile = manager.profile_dir("spare");
    let lines = [
        meta(THREAD, None, json!("cli")),
        turn("turn-a", "gpt-6-sol"),
        record("response-a", "turn-a", AT, usage(100, 0, 0, 10)),
    ];
    write_lines(&plain.join("sessions/2030/01/01/one.jsonl"), &lines);
    write_lines(
        &profile.join("sessions/2030/01/01/one.jsonl"),
        &[
            meta(ROOT, None, json!("cli")),
            turn("turn-a", "gpt-6-sol"),
            record("response-b", "turn-a", AT, usage(100, 0, 0, 10)),
        ],
    );
    write_lines(&profile.join("projects/demo/fake.jsonl"),&[r#"{"type":"assistant","sessionId":"fake","timestamp":"2030-01-01T12:00:00Z","message":{"id":"x","model":"gpt-6-sol","usage":{"input_tokens":9}}}"#.into()]);
    write_lines(
        &manager
            .profile_dir("old")
            .join("sessions/2030/01/01/one.jsonl"),
        &lines,
    );
    let store = super::store(&manager, Some(temp.path().join("ledger"))).unwrap();
    assert_eq!(store.sources.len(), 1);
    assert_eq!(store.codex_sources.len(), 2);
    assert_eq!(store.codex_sources[0].profile, "codex:default");
    store.ingest().unwrap();
    let ledger = store.load().unwrap();
    assert_eq!(ledger.requests.len(), 2);
    assert!(
        ledger
            .requests
            .iter()
            .any(|row| row.profile == "codex:default")
    );
    assert!(ledger.requests.iter().any(|row| row.profile == "spare"));
    assert!(
        !ledger
            .requests
            .iter()
            .any(|row| row.profile == "old" || row.session == "fake")
    );
    assert_eq!(store.purge_profile("spare").unwrap(), 1);
    let ledger = store.load().unwrap();
    assert_eq!(ledger.requests.len(), 1);
    assert!(
        !ledger
            .cursors
            .files
            .keys()
            .any(|path| path.starts_with(profile.to_str().unwrap()))
    );
}

#[test]
fn codex_index_tracks_renames_rewrites_and_bad_lines() {
    // Known-bad: an appended or rewritten index leaves the first thread name showing.
    let fixture = Fixture::new();
    write_lines(
        &fixture.file("one"),
        &[
            meta(THREAD, None, json!("cli")),
            record("response-a", "missing", AT, usage(1, 0, 0, 1)),
        ],
    );
    let index = fixture.home.join("session_index.jsonl");
    let title =
        |name: &str| format!(r#"{{"id":"{THREAD}","thread_name":"{name}","updated_at":"{AT}"}}"#);
    write_lines(&index, &[title("First title")]);
    fixture.store.ingest().unwrap();
    append_lines(&index, &[title("Later title")]);
    fixture.store.ingest().unwrap();
    let ledger = fixture.store.load().unwrap();
    assert_eq!(
        ledger.sessions[0].titles.last().unwrap().value,
        "Later title"
    );
    write_lines(&index, &[title("Other title")]);
    File::open(&index)
        .unwrap()
        .set_modified(
            UNIX_EPOCH + Duration::from_secs(at("2030-01-02T00:00:00Z").timestamp() as u64),
        )
        .unwrap();
    fixture.store.ingest().unwrap();
    let ledger = fixture.store.load().unwrap();
    assert_eq!(ledger.sessions[0].titles.len(), 1);
    assert_eq!(ledger.sessions[0].titles[0].value, "Other title");
    let tree = super::report::run(
        &fixture.store,
        &super::report::Options {
            since: "all".into(),
            profile: None,
            by: None,
            json: false,
            explain: None,
            unattributed: false,
        },
        at("2030-01-02T00:00:00Z"),
    )
    .unwrap();
    assert!(tree.contains("Other title"));
    write_lines(&index, &["not json".into()]);
    fixture.store.ingest().unwrap();
    assert_eq!(
        fixture.store.load().unwrap().sessions[0].titles[0].value,
        "Other title"
    );
}

#[test]
fn codex_ingest_never_changes_source_bytes_or_mtime() {
    // Known-bad: ingest writes a cursor, lock, or renamed file in the Codex home.
    let fixture = Fixture::new();
    let path = fixture.file("one");
    write_lines(
        &path,
        &[
            meta(THREAD, None, json!("cli")),
            record("response-a", "missing", AT, usage(1, 0, 0, 1)),
        ],
    );
    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fixture.store.ingest().unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_dir(&fixture.home).unwrap().count(), 1);
    assert!(fixture.temp.path().join("ledger/cursors.json").exists());
}

#[cfg(unix)]
#[test]
fn codex_source_does_not_follow_a_linked_sessions_root() {
    // Known-bad: walking a linked sessions root reads files outside the Codex home.
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.temp.path().join("outside");
    write_lines(
        &outside.join("2030/01/01/one.jsonl"),
        &[
            meta(THREAD, None, json!("cli")),
            record("response-a", "missing", AT, usage(1, 0, 0, 1)),
        ],
    );
    fs::create_dir_all(&fixture.home).unwrap();
    symlink(&outside, fixture.home.join("sessions")).unwrap();
    assert_eq!(fixture.store.ingest().unwrap().new_requests, 0);
}

#[test]
fn codex_exact_threshold_uses_short_then_long_on_each_card() {
    // Known-bad: >= 272,000 selects the long card at the boundary.
    let rates = seed();
    for (model, standard, fast, long_standard, long_fast) in [
        ("gpt-6-sol", 2.0, 4.0, 4.0, 8.0),
        ("gpt-6-astra", 10.0, 20.0, 20.0, 40.0),
    ] {
        for (tier, short_price, long_price) in [
            ("default", standard, long_standard),
            ("priority", fast, long_fast),
        ] {
            let mut reader = Reader::default();
            reader
                .line(meta(THREAD, None, json!("cli")).as_bytes(), "sample")
                .unwrap();
            reader
                .line(turn("turn-a", model).as_bytes(), "sample")
                .unwrap();
            reader.line(settings(tier).as_bytes(), "sample").unwrap();
            for (tokens, expected) in [(272_000, short_price), (272_001, long_price)] {
                let request = reader
                    .line(
                        record(
                            &format!("response-{tokens}"),
                            "turn-a",
                            AT,
                            usage(tokens, 0, 0, 0),
                        )
                        .as_bytes(),
                        "sample",
                    )
                    .unwrap()
                    .unwrap();
                let prompt = rates.prompt_tier(&request).unwrap();
                assert_eq!(prompt.long, tokens > 272_000);
                assert_eq!(
                    rates
                        .price_for(model, request.speed.as_deref(), Some(prompt))
                        .unwrap()
                        .input,
                    expected
                );
            }
        }
    }
}

#[test]
fn codex_priority_default_flex_and_claude_standard_prices() {
    // Known-bad: flex silently falls back to a standard card.
    let rates = seed();
    let mut reader = Reader::default();
    reader
        .line(meta(THREAD, None, json!("cli")).as_bytes(), "sample")
        .unwrap();
    reader
        .line(turn("turn-a", "gpt-6-sol").as_bytes(), "sample")
        .unwrap();
    reader
        .line(settings("priority").as_bytes(), "sample")
        .unwrap();
    let mut request = reader
        .line(
            record("response-a", "turn-a", AT, usage(1_000_000, 0, 0, 0)).as_bytes(),
            "sample",
        )
        .unwrap()
        .unwrap();
    assert_eq!(request.service_tier.as_deref(), Some("priority"));
    assert_eq!(request.speed.as_deref(), Some("fast"));
    assert_eq!(cost(&request, &rates).unwrap().input, 8.0);
    reader
        .line(settings("default").as_bytes(), "sample")
        .unwrap();
    request = reader
        .line(
            record("response-b", "turn-a", AT, usage(1_000_000, 0, 0, 0)).as_bytes(),
            "sample",
        )
        .unwrap()
        .unwrap();
    assert_eq!(request.speed, None);
    assert_eq!(cost(&request, &rates).unwrap().input, 4.0);
    reader.line(settings("flex").as_bytes(), "sample").unwrap();
    request = reader
        .line(
            record("response-c", "turn-a", AT, usage(1_000_000, 0, 0, 0)).as_bytes(),
            "sample",
        )
        .unwrap()
        .unwrap();
    assert_eq!(request.speed.as_deref(), Some("flex"));
    assert!(cost(&request, &rates).is_none());
    request.model = "claude-sonnet-5".into();
    request.input = 1_000_000;
    request.speed = Some("standard".into());
    assert_eq!(cost(&request, &rates).unwrap().input, 2.0);
    request.speed = None;
    assert_eq!(cost(&request, &rates).unwrap().input, 2.0);
}

#[test]
fn codex_seed_cards_source_and_existing_rates_bytes() {
    // Known-bads: wrong cached input, doubled long output, wrong cache writes,
    // or overwriting an existing rate file.
    let rates = seed();
    assert_eq!(
        rates.source,
        "Anthropic first-party list prices, https://platform.claude.com/docs/en/about-claude/pricing (read 2026-10-10); OpenAI API list prices, https://developers.openai.com/api/docs/pricing (read 2026-10-14)"
    );
    for (model, cards) in [
        (
            "gpt-6-sol",
            [
                [2.0, 10.0, 2.5, 2.5, 0.2],
                [4.0, 20.0, 5.0, 5.0, 0.4],
                [4.0, 15.0, 5.0, 5.0, 0.4],
                [8.0, 30.0, 10.0, 10.0, 0.8],
            ],
        ),
        (
            "gpt-6-astra",
            [
                [10.0, 50.0, 12.5, 12.5, 1.0],
                [20.0, 100.0, 25.0, 25.0, 2.0],
                [20.0, 75.0, 25.0, 25.0, 2.0],
                [40.0, 150.0, 50.0, 50.0, 4.0],
            ],
        ),
    ] {
        let rate = &rates.models[model];
        let long = rate.long_prompt.as_ref().unwrap();
        assert_eq!(long.over_tokens, 272_000);
        for (price, expected) in [
            (&rate.standard, cards[0]),
            (rate.fast.as_ref().unwrap(), cards[1]),
            (&long.standard, cards[2]),
            (long.fast.as_ref().unwrap(), cards[3]),
        ] {
            assert_eq!(
                [
                    price.input,
                    price.output,
                    price.cache_write_5m,
                    price.cache_write_1h,
                    price.cache_read
                ],
                expected
            );
        }
    }
    assert!(!rates.models.contains_key("codex-auto-review"));
    let temp = TempDir::new().unwrap();
    let bytes = b"{\"source\":\"invented\",\"unit\":\"USD per million tokens\",\"models\":{}}";
    fs::write(temp.path().join("rates.json"), bytes).unwrap();
    load_or_seed(temp.path()).unwrap();
    assert_eq!(fs::read(temp.path().join("rates.json")).unwrap(), bytes);
}

#[test]
fn claude_only_ledger_and_reports_match_claude_source_path() {
    // Known-bad: adding Codex sources changes a Claude-only figure or persisted file.
    let temp = TempDir::new().unwrap();
    let manager =
        ProfileManager::with_paths(temp.path().join("switch"), temp.path().join(".claude"))
            .unwrap();
    let claude = manager.profile_dir("work");
    fs::create_dir_all(&claude).unwrap();
    let mut registry = Registry::default();
    registry.profiles.insert(
        "work".into(),
        Profile {
            name: "work".into(),
            tool: Tool::Claude,
            email: Some("work@example.com".into()),
            added: at(AT),
            last_used: None,
        },
    );
    fs::write(
        manager.base_dir.join("registry.json"),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    write_lines(&claude.join("projects/demo/session.jsonl"), &[r#"{"type":"assistant","timestamp":"2030-01-01T12:00:00Z","sessionId":"claude-session","requestId":"req-one","message":{"id":"msg-one","model":"claude-sonnet-5","usage":{"input_tokens":100000,"output_tokens":10000}}}"#.into()]);
    let modern = super::store(&manager, Some(temp.path().join("modern"))).unwrap();
    let legacy = Store::new(temp.path().join("claude-only"), modern.sources.clone());
    modern.ingest().unwrap();
    legacy.ingest().unwrap();
    let normalize = |path: &Path| {
        fs::read_to_string(path)
            .unwrap()
            .split_inclusive('\n')
            .filter(|line| !line.trim_start().starts_with("\"generated_at\":"))
            .collect::<String>()
    };
    for name in ["hourly.json", "chats.json", "summary.json", "cursors.json"] {
        assert_eq!(
            normalize(&modern.dir.join(name)),
            normalize(&legacy.dir.join(name)),
            "{name}"
        );
    }
    let options = super::report::Options {
        since: "all".into(),
        profile: None,
        by: None,
        json: false,
        explain: None,
        unattributed: false,
    };
    let now = at("2030-01-02T00:00:00Z");
    let modern_report = super::report::run(&modern, &options, now).unwrap();
    assert!(modern_report.contains("$0.30"), "{modern_report}");
    assert_eq!(
        modern_report,
        super::report::run(&legacy, &options, now).unwrap()
    );
    for by in ["profile", "workspace", "project", "session", "model", "day"] {
        let options = super::report::Options {
            since: "all".into(),
            profile: None,
            by: Some(by.into()),
            json: false,
            explain: None,
            unattributed: false,
        };
        assert_eq!(
            super::report::run(&modern, &options, now).unwrap(),
            super::report::run(&legacy, &options, now).unwrap(),
            "--by {by}"
        );
    }
    for (json, explain, unattributed) in [
        (true, None, false),
        (false, Some("claude-session"), false),
        (false, None, true),
    ] {
        let options = super::report::Options {
            since: "all".into(),
            profile: None,
            by: None,
            json,
            explain: explain.map(str::to_string),
            unattributed,
        };
        assert_eq!(
            super::report::run(&modern, &options, now).unwrap(),
            super::report::run(&legacy, &options, now).unwrap()
        );
    }
    assert_eq!(
        super::report::verify(&modern).unwrap(),
        super::report::verify(&legacy).unwrap()
    );
}
