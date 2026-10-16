//! Offline identity and plan limits from a Codex home. Token bytes never leave this module.

use crate::limits::{Limits, Snapshot, Window, WindowGroup};
use crate::usage::ledger::transcript_files;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[cfg(test)]
thread_local! {
    static READ_ATTEMPTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub fn reset_read_stats() {
    READ_ATTEMPTS.set(0);
}

#[cfg(test)]
pub fn read_stats() -> usize {
    READ_ATTEMPTS.get()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub email: String,
    pub plan_type: Option<String>,
}

pub fn identity_from_auth(bytes: &[u8]) -> Option<Identity> {
    let auth: Value = serde_json::from_slice(bytes).ok()?;
    let token = auth.get("tokens")?.get("id_token")?.as_str()?;
    let claims = jwt_claims(token)?;
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .filter(|email| !email.trim().is_empty())
        .or_else(|| {
            claims
                .get("https://api.openai.com/profile")?
                .get("email")?
                .as_str()
                .filter(|email| !email.trim().is_empty())
        })?
        .trim();
    if email.is_empty() {
        return None;
    }
    let plan_type = claims
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_plan_type"))
        .and_then(Value::as_str)
        .filter(|plan| !plan.trim().is_empty())
        .map(str::to_string);
    Some(Identity {
        email: email.to_string(),
        plan_type,
    })
}

pub fn account_id_from_auth(bytes: &[u8]) -> Option<String> {
    let auth: Value = serde_json::from_slice(bytes).ok()?;
    let token = auth.get("tokens")?.get("id_token")?.as_str()?;
    jwt_claims(token)?
        .get("https://api.openai.com/auth")?
        .get("chatgpt_account_id")?
        .as_str()
        .map(str::to_string)
}

/// Read only session tails. Codex homes are never modified by this reader.
pub fn read_limits(codex_home: &Path, account_id: Option<&str>) -> Limits {
    let mut paths = Vec::new();
    let root = codex_home.join("sessions");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if !metadata.is_dir() => return Limits::NoSnapshot,
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Limits::NoSnapshot,
        Err(_) => return Limits::Unreadable,
    }
    if let Err(error) = transcript_files(&root, &mut paths) {
        return if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            Limits::NoSnapshot
        } else {
            Limits::Unreadable
        };
    }
    let mut files = paths
        .into_iter()
        .filter_map(|path| {
            fs::metadata(&path)
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map(|modified| (modified, path))
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| b.cmp(a));

    let mut best: Option<(DateTime<Utc>, Value, PathBuf)> = None;
    for (modified, path) in files {
        if best
            .as_ref()
            .is_some_and(|(at, _, _)| DateTime::<Utc>::from(modified) < *at)
        {
            break;
        }
        #[cfg(test)]
        READ_ATTEMPTS.set(READ_ATTEMPTS.get() + 1);
        let Some((at, rate_limits)) = last_limit_event(&path) else {
            continue;
        };
        if best.as_ref().is_none_or(|(previous, _, _)| at > *previous) {
            best = Some((at, rate_limits, path));
        }
    }

    let Some((fetched_at, rate_limits, path)) = best else {
        return Limits::NoSnapshot;
    };
    if let (Some(expected), Some(actual)) = (account_id, creator_account_id(&path))
        && expected != actual
    {
        return Limits::AccountMismatch;
    }
    let windows = ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| rate_limits.get(key).and_then(parse_limit_window))
        .collect();
    Limits::Snapshot(Snapshot {
        fetched_at,
        windows,
        window_started_at: None,
    })
}

fn last_limit_event(path: &Path) -> Option<(DateTime<Utc>, Value)> {
    const TAIL: u64 = 1 << 20;
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL);
    let cut_first = if start > 0 {
        let mut before = [0];
        file.seek(SeekFrom::Start(start - 1)).ok()?;
        file.read_exact(&mut before).ok()?;
        before[0] != b'\n'
    } else {
        false
    };
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = vec![0; (len - start) as usize];
    file.read_exact(&mut tail).ok()?;
    let first = if cut_first {
        tail.iter().position(|byte| *byte == b'\n')? + 1
    } else {
        0
    };
    let end = tail.iter().rposition(|byte| *byte == b'\n')?;
    if first > end {
        return None;
    }
    for line in tail[first..end].rsplit(|byte| *byte == b'\n') {
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if record.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let Some(payload) = record.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let Some(rate_limits) = payload.get("rate_limits") else {
            continue;
        };
        if rate_limits.get("limit_id").and_then(Value::as_str) != Some("codex") {
            continue;
        }
        let Some(at) = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        else {
            continue;
        };
        return Some((at.with_timezone(&Utc), rate_limits.clone()));
    }
    None
}

fn creator_account_id(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut first = Vec::new();
    BufReader::new(file.take(64 * 1024))
        .read_until(b'\n', &mut first)
        .ok()?;
    if !first.ends_with(b"\n") {
        return None;
    }
    let record: Value = serde_json::from_slice(&first).ok()?;
    if record.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    record
        .get("payload")?
        .get("creator_account_id")?
        .as_str()
        .map(str::to_string)
}

fn parse_limit_window(value: &Value) -> Option<Window> {
    let minutes = value.get("window_minutes")?.as_i64()?;
    let percent = value.get("used_percent")?.as_f64()?;
    let (group, kind) = match minutes {
        300 => (WindowGroup::Session, "session".to_string()),
        10_080 => (WindowGroup::Weekly, "weekly_all".to_string()),
        other => (
            WindowGroup::Other(String::new()),
            format!("window_{other}min"),
        ),
    };
    Some(Window {
        group,
        kind,
        percent,
        severity: None,
        resets_at: value
            .get("resets_at")
            .and_then(Value::as_i64)
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0)),
        is_active: None,
    })
}

pub(crate) fn jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if header.is_empty() || payload.is_empty() || signature.is_empty() {
        return None;
    }
    serde_json::from_slice(&decode_base64url(payload)?).ok()
}

fn decode_base64url(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let unpadded = bytes
        .iter()
        .position(|byte| *byte == b'=')
        .unwrap_or(bytes.len());
    let padding = bytes.len() - unpadded;
    if padding > 2 || bytes[unpadded..].iter().any(|byte| *byte != b'=') {
        return None;
    }
    if unpadded % 4 == 1 || (padding > 0 && !bytes.len().is_multiple_of(4)) {
        return None;
    }
    let mut output = Vec::with_capacity(unpadded * 3 / 4);
    let mut bits = 0u32;
    let mut count = 0u8;
    for &byte in &bytes[..unpadded] {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(digit);
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    if bits != 0 {
        return None;
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(data: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut encoded = String::new();
        for chunk in data.chunks(3) {
            let number = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            encoded.push(ALPHABET[((number >> 18) & 63) as usize] as char);
            encoded.push(ALPHABET[((number >> 12) & 63) as usize] as char);
            if chunk.len() > 1 {
                encoded.push(ALPHABET[((number >> 6) & 63) as usize] as char);
            }
            if chunk.len() > 2 {
                encoded.push(ALPHABET[(number & 63) as usize] as char);
            }
        }
        encoded
    }

    #[test]
    fn identity_reads_synthetic_claims_without_padding() {
        // Known-bad: treating an unpadded JWT payload as invalid or missing its plan claim.
        let claims = br#"{"email":"user@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"plus"}} "#;
        assert_ne!(encode(claims).len() % 4, 0);
        let auth = format!(
            r#"{{"tokens":{{"id_token":"header.{}.signature"}}}}"#,
            encode(claims)
        );
        assert_eq!(
            identity_from_auth(auth.as_bytes()),
            Some(Identity {
                email: "user@example.com".into(),
                plan_type: Some("plus".into()),
            })
        );
    }

    #[test]
    fn identity_decodes_both_base64url_only_characters() {
        // Known-bad: decoding the URL alphabet as standard Base64 rejects '-' and '_'.
        let mut claims = String::from(
            r#"{"email":"user@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"plus"},"noise":""#,
        );
        while claims.len() % 3 != 0 {
            claims.push(' ');
        }
        claims.push_str("~~~???");
        claims.push_str("\"}");
        let payload = encode(claims.as_bytes());
        assert!(payload.contains('-') && payload.contains('_'), "{payload}");
        let auth = format!(r#"{{"tokens":{{"id_token":"h.{payload}.s"}}}}"#);
        assert_eq!(
            identity_from_auth(auth.as_bytes()),
            Some(Identity {
                email: "user@example.com".into(),
                plan_type: Some("plus".into()),
            })
        );
    }

    #[test]
    fn identity_uses_profile_email_fallback() {
        // Known-bad: only the top-level email is accepted.
        let claims = br#"{"https://api.openai.com/profile":{"email":"fallback@example.com"}}"#;
        let auth = format!(r#"{{"tokens":{{"id_token":"h.{}.s"}}}}"#, encode(claims));
        assert_eq!(
            identity_from_auth(auth.as_bytes()).unwrap().email,
            "fallback@example.com"
        );
    }

    #[test]
    fn identity_falls_back_when_top_level_email_is_empty() {
        // Known-bad: an empty top-level email masks a populated profile claim.
        let claims =
            br#"{"email":" ","https://api.openai.com/profile":{"email":"fallback@example.com"}}"#;
        let auth = format!(r#"{{"tokens":{{"id_token":"h.{}.s"}}}}"#, encode(claims));
        assert_eq!(
            identity_from_auth(auth.as_bytes()).unwrap().email,
            "fallback@example.com"
        );
    }

    #[test]
    fn identity_rejects_missing_token() {
        // Known-bad: missing token data is treated as an account.
        assert_eq!(identity_from_auth(br#"{}"#), None);
    }

    #[test]
    fn identity_rejects_bad_segment_count() {
        // Known-bad: indexing JWT segments panics when the signature is missing.
        assert_eq!(
            identity_from_auth(br#"{"tokens":{"id_token":"bad"}}"#),
            None
        );
    }

    #[test]
    fn identity_requires_exactly_three_jwt_segments() {
        // Known-bad: extra segments after the signature are ignored.
        let payload = encode(br#"{"email":"user@example.com"}"#);
        for token in [format!("h.{payload}"), format!("h.{payload}.s.extra")] {
            let auth = format!(r#"{{"tokens":{{"id_token":"{token}"}}}}"#);
            assert_eq!(identity_from_auth(auth.as_bytes()), None, "{token}");
        }
    }

    #[test]
    fn identity_rejects_empty_jwt_segments() {
        // Known-bad: a token with an empty header or signature is treated as a valid JWT.
        let payload = encode(br#"{"email":"user@example.com"}"#);
        for token in [format!(".{payload}.s"), format!("h.{payload}.")] {
            let auth = format!(r#"{{"tokens":{{"id_token":"{token}"}}}}"#);
            assert_eq!(identity_from_auth(auth.as_bytes()), None);
        }
    }

    #[test]
    fn identity_rejects_non_json_payload() {
        // Known-bad: a decoded non-JSON payload is accepted or panics during claim access.
        assert_eq!(
            identity_from_auth(br#"{"tokens":{"id_token":"h.bm90anNvbg.s"}}"#),
            None
        );
    }

    #[test]
    fn identity_rejects_invalid_base64url() {
        // Known-bad: invalid base64url characters silently decode into an identity.
        assert_eq!(
            identity_from_auth(br#"{"tokens":{"id_token":"h.@@.s"}}"#),
            None
        );
    }

    #[test]
    fn account_id_comes_only_from_id_token_claim() {
        // Known-bad: an unrelated account_id field is trusted instead of the id token.
        let claims = br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"00000000-0000-4000-8000-000000000001"}}"#;
        let auth = format!(
            r#"{{"tokens":{{"id_token":"h.{}.s","account_id":"00000000-0000-4000-8000-000000000002","access_token":"ignored","refresh_token":"ignored"}}}}"#,
            encode(claims)
        );
        assert_eq!(
            account_id_from_auth(auth.as_bytes()).as_deref(),
            Some("00000000-0000-4000-8000-000000000001")
        );
    }
}
