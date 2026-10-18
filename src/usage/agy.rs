//! Stream Antigravity model steps into the ledger without changing their transcripts.

use super::parse::Request;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeSet;

pub struct Reader<'a> {
    conversation: &'a str,
}

impl<'a> Reader<'a> {
    pub fn new(conversation: &'a str) -> Self {
        Self { conversation }
    }

    pub fn line(&self, line: &[u8], profile: &str) -> Result<Option<Request>, ()> {
        let row: Value = serde_json::from_slice(line).map_err(|_| ())?;
        if row.get("source").and_then(Value::as_str) != Some("MODEL") {
            return Ok(None);
        }
        let number = |name| row.get(name).and_then(Value::as_u64).ok_or(());
        let (input, output, cache_read, step) = (
            number("input_tokens")?,
            number("output_tokens")?,
            number("cache_read_tokens")?,
            number("step_index")?,
        );
        let at = row
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
            .ok_or(())?
            .with_timezone(&Utc);
        let key =
            serde_json::to_string(&("antigravity", self.conversation, step)).map_err(|_| ())?;
        Ok(Some(Request {
            key,
            message_id: None,
            time: at,
            profile: profile.to_string(),
            session: self.conversation.to_string(),
            agent_id: None,
            model: String::new(),
            input,
            output,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read,
            web_searches: 0,
            web_fetches: 0,
            speed: None,
            service_tier: None,
            inference_geo: None,
            version: None,
            cwd: None,
            tool_touches: Vec::new(),
            cwd_project: None,
            workspace: None,
            touched_projects: Vec::new(),
            touched_workspaces: Vec::new(),
            touch_count: 0,
            seen_tool_use_ids: BTreeSet::new(),
            sidechain: false,
        }))
    }
}
