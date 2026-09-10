//! Pure parser for one Claude Code transcript line.
//!
//! Mirrors T3 Code's `parseClaudeLine`: keep only `assistant` records that
//! carry a `message.usage` block, and read defensively — a malformed or
//! non-usage line yields `None` rather than an error, because transcripts
//! interleave many line shapes and a scan must tolerate all of them.

use serde::Deserialize;

use super::record::{TokenTotals, UsageRecord};

/// A cheap substring gate the shell can apply before calling [`parse_claude_line`].
///
/// A line without the literal `"usage"` can never produce a record, and this
/// check is roughly an order of magnitude cheaper than a full JSON parse. Kept
/// here so the rule lives next to the parser it guards.
#[must_use]
pub fn might_carry_usage(line: &str) -> bool {
    line.contains("\"usage\"")
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "costUSD")]
    cost_usd: Option<f64>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    model: Option<String>,
    id: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// Parses one transcript line into a [`UsageRecord`], or `None` when the line
/// is not a billable assistant turn.
///
/// Returns `None` — never an error — for malformed JSON, non-`assistant`
/// records, a missing `usage` block, an unparseable timestamp, or a blank
/// model, matching the tolerance a streaming scan needs.
#[must_use]
pub fn parse_claude_line(line: &str) -> Option<UsageRecord> {
    let parsed: Line = serde_json::from_str(line).ok()?;

    if parsed.kind.as_deref() != Some("assistant") {
        return None;
    }
    let message = parsed.message?;
    let usage = message.usage?;

    let timestamp_ms = parse_timestamp_ms(parsed.timestamp.as_deref()?)?;

    let model = message.model.filter(|m| !m.is_empty())?;

    // Matches ccusage: prefer the message/request pair, fall back to whichever
    // half exists. A record with neither cannot be de-duplicated.
    let dedupe_key = match (&message.id, &parsed.request_id) {
        (None, None) => None,
        (id, req) => Some(format!(
            "{}:{}",
            id.as_deref().unwrap_or(""),
            req.as_deref().unwrap_or("")
        )),
    };

    Some(UsageRecord {
        timestamp_ms,
        model,
        session_id: parsed.session_id.unwrap_or_default(),
        project: parsed.cwd.unwrap_or_default(),
        totals: TokenTotals {
            uncached_input: usage.input_tokens,
            cached_input: usage.cache_read_input_tokens,
            cache_creation: usage.cache_creation_input_tokens,
            output: usage.output_tokens,
        },
        reported_cost_usd: parsed.cost_usd.filter(|c| c.is_finite()),
        dedupe_key,
    })
}

/// Parses an RFC 3339 timestamp into Unix milliseconds.
fn parse_timestamp_ms(raw: &str) -> Option<i64> {
    raw.parse::<jiff::Timestamp>()
        .ok()
        .map(|ts| ts.as_millisecond())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record shaped like a real Claude Code assistant turn (token counts
    /// and field layout taken from an actual `~/.claude/projects` transcript).
    const ASSISTANT: &str = r#"{"type":"assistant","requestId":"req_abc","sessionId":"sess_1","timestamp":"2026-09-10T12:00:00.000Z","message":{"model":"claude-opus-4-8","id":"msg_1","usage":{"input_tokens":2,"cache_read_input_tokens":26619,"cache_creation_input_tokens":11745,"output_tokens":138}}}"#;

    #[test]
    fn parses_a_real_assistant_turn() {
        let r = parse_claude_line(ASSISTANT).expect("should parse");
        assert_eq!(r.model, "claude-opus-4-8");
        assert_eq!(r.session_id, "sess_1");
        assert_eq!(r.dedupe_key.as_deref(), Some("msg_1:req_abc"));
        assert_eq!(r.totals.uncached_input, 2);
        assert_eq!(r.totals.cached_input, 26619);
        assert_eq!(r.totals.cache_creation, 11745);
        assert_eq!(r.totals.output, 138);
        assert_eq!(r.reported_cost_usd, None);
        // 2026-09-10T12:00:00Z
        assert_eq!(r.timestamp_ms, 1_789_041_600_000);
    }

    #[test]
    fn skips_non_assistant_and_malformed_lines() {
        assert!(parse_claude_line(r#"{"type":"user","message":{}}"#).is_none());
        assert!(parse_claude_line(r#"{"type":"assistant","message":{"model":"x"}}"#).is_none());
        assert!(parse_claude_line("not json at all").is_none());
        assert!(parse_claude_line("").is_none());
    }

    #[test]
    fn missing_token_fields_default_to_zero() {
        let line = r#"{"type":"assistant","timestamp":"2026-09-10T12:00:00Z","message":{"model":"claude-sonnet-5","id":"m","usage":{"output_tokens":10}}}"#;
        let r = parse_claude_line(line).expect("should parse");
        assert_eq!(r.totals.output, 10);
        assert_eq!(r.totals.uncached_input, 0);
        assert_eq!(r.totals.cache_creation, 0);
    }

    #[test]
    fn keeps_a_reported_cost_when_present() {
        let line = r#"{"type":"assistant","timestamp":"2026-09-10T12:00:00Z","costUSD":0.42,"message":{"model":"claude-opus-4-8","id":"m","usage":{"output_tokens":10}}}"#;
        let r = parse_claude_line(line).expect("should parse");
        assert_eq!(r.reported_cost_usd, Some(0.42));
    }

    #[test]
    fn might_carry_usage_gates_cheaply() {
        assert!(might_carry_usage(ASSISTANT));
        assert!(!might_carry_usage(r#"{"type":"user","message":{}}"#));
    }
}
