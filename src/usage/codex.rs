//! Stream Codex session records into the usage ledger without changing their home.

use super::parse::Request;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone)]
struct Turn {
    model: String,
    cwd: Option<String>,
}

struct SessionContext {
    id: String,
    version: Option<String>,
    cwd: Option<String>,
    sidechain: bool,
}

#[derive(Default)]
pub struct Reader {
    session: Option<SessionContext>,
    turns: HashMap<String, Turn>,
    latest_turn: Option<Turn>,
    tier: Option<String>,
    saw_record: bool,
    last_total: Option<Value>,
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn timestamp(row: &Value) -> Result<(String, DateTime<Utc>), ()> {
    let raw = string(row, "timestamp").ok_or(())?;
    let at = DateTime::parse_from_rfc3339(raw)
        .map_err(|_| ())?
        .with_timezone(&Utc);
    Ok((raw.to_string(), at))
}

pub fn relevant(line: &[u8]) -> bool {
    const NEEDLES: [&[u8]; 5] = [
        b"\"type\":\"session_meta\"",
        b"\"type\":\"turn_context\"",
        b"\"type\":\"token_usage_record\"",
        b"\"type\":\"token_count\"",
        b"\"type\":\"thread_settings_applied\"",
    ];
    let head = &line[..line.len().min(160)];
    NEEDLES
        .iter()
        .any(|needle| head.windows(needle.len()).any(|part| part == *needle))
}

impl Reader {
    pub fn line(&mut self, line: &[u8], profile: &str) -> Result<Option<Request>, ()> {
        let row: Value = serde_json::from_slice(line).map_err(|_| ())?;
        let payload = row.get("payload").ok_or(())?;
        match string(&row, "type") {
            Some("session_meta") => {
                if self.session.is_none() {
                    let id = string(payload, "id")
                        .filter(|id| !id.is_empty())
                        .ok_or(())?;
                    self.session = Some(SessionContext {
                        id: string(payload, "session_id")
                            .filter(|root| !root.is_empty())
                            .unwrap_or(id)
                            .to_string(),
                        version: string(payload, "cli_version").map(str::to_string),
                        cwd: string(payload, "cwd").map(str::to_string),
                        sidechain: payload.get("source").is_some_and(Value::is_object),
                    });
                }
            }
            Some("turn_context") => {
                let model = string(payload, "model")
                    .filter(|model| !model.is_empty())
                    .ok_or(())?;
                let turn = Turn {
                    model: model.to_string(),
                    cwd: string(payload, "cwd").map(str::to_string),
                };
                if let Some(id) = string(payload, "turn_id") {
                    self.turns.insert(id.to_string(), turn.clone());
                }
                self.latest_turn = Some(turn);
            }
            Some("event_msg") if string(payload, "type") == Some("thread_settings_applied") => {
                self.tier = payload
                    .get("thread_settings")
                    .and_then(|settings| string(settings, "service_tier"))
                    .map(str::to_string);
            }
            Some("token_usage_record") => {
                self.saw_record = true;
                let response = string(payload, "response_id").ok_or(())?;
                let usage = payload
                    .get("usage")
                    .filter(|value| value.is_object())
                    .ok_or(())?;
                let (_, at) = timestamp(&row)?;
                let key = serde_json::to_string(&("codex", response)).map_err(|_| ())?;
                let turn = string(payload, "turn_id")
                    .and_then(|id| self.turns.get(id))
                    .or(self.latest_turn.as_ref());
                return self.request(profile, at, key, usage, turn);
            }
            Some("event_msg") if string(payload, "type") == Some("token_count") => {
                let info = payload.get("info").ok_or(())?;
                if info.is_null() {
                    return Ok(None);
                }
                let total = info
                    .get("total_token_usage")
                    .filter(|value| value.is_object())
                    .ok_or(())?;
                let usage = info
                    .get("last_token_usage")
                    .filter(|value| value.is_object())
                    .ok_or(())?;
                let changed = self.last_total.as_ref() != Some(total);
                self.last_total = Some(total.clone());
                if !changed || self.saw_record {
                    return Ok(None);
                }
                let (raw, at) = timestamp(&row)?;
                let session = self.session.as_ref().ok_or(())?;
                let key = serde_json::to_string(&(
                    "codex-count",
                    session.id.as_str(),
                    raw.as_str(),
                    number(total, "total_tokens"),
                ))
                .map_err(|_| ())?;
                return self.request(profile, at, key, usage, self.latest_turn.as_ref());
            }
            _ => {}
        }
        Ok(None)
    }

    fn request(
        &self,
        profile: &str,
        time: DateTime<Utc>,
        key: String,
        usage: &Value,
        turn: Option<&Turn>,
    ) -> Result<Option<Request>, ()> {
        let session = self.session.as_ref().ok_or(())?;
        let input = number(usage, "input_tokens");
        let cache_read = number(usage, "cached_input_tokens");
        let cache_write = number(usage, "cache_write_input_tokens");
        let speed = match self.tier.as_deref() {
            Some("priority" | "fast") => Some("fast".to_string()),
            Some("" | "default" | "auto") | None => None,
            Some(other) => Some(other.to_string()),
        };
        Ok(Some(Request {
            key,
            message_id: None,
            time,
            profile: profile.to_string(),
            session: session.id.clone(),
            agent_id: None,
            model: turn
                .map_or("(unknown)", |turn| turn.model.as_str())
                .to_string(),
            input: input.saturating_sub(cache_read).saturating_sub(cache_write),
            output: number(usage, "output_tokens"),
            cache_write_5m: cache_write,
            cache_write_1h: 0,
            cache_read,
            web_searches: 0,
            web_fetches: 0,
            speed,
            service_tier: self.tier.clone(),
            inference_geo: None,
            version: session.version.clone(),
            cwd: turn
                .and_then(|turn| turn.cwd.as_ref())
                .or_else(|| self.latest_turn.as_ref().and_then(|turn| turn.cwd.as_ref()))
                .cloned()
                .or_else(|| session.cwd.clone()),
            tool_touches: Vec::new(),
            cwd_project: None,
            workspace: None,
            touched_projects: Vec::new(),
            touched_workspaces: Vec::new(),
            touch_count: 0,
            seen_tool_use_ids: BTreeSet::new(),
            sidechain: session.sidechain,
        }))
    }
}
