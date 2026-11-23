//! What a session's first and last records say, read from bounded byte
//! windows. Pure: no file access.
//!
//! Formats, verified on real files by key names and value types only:
//!
//! - **Claude Code** writes one JSON object per line. Conversation records
//!   carry `type` (`user`, `assistant`, `attachment`, …), `sessionId`, `cwd`
//!   and an ISO-8601 `timestamp`. A person's prompt is a `user` record whose
//!   `message.content` is a string (or `text` parts); tool results are
//!   `tool_result` parts, and `isMeta` marks injected text.
//! - **Codex** starts with a `session_meta` record whose `payload` has `id`,
//!   `cwd` and `timestamp`; `turn_context` payloads repeat `cwd`. Prompts are
//!   `response_item` payloads of type `message` with role `user` and
//!   `input_text` parts. The first ones are injected instructions and
//!   environment context, which start with `<` or `# AGENTS.md`.
//!
//! A window may end in the middle of a line: that line is ignored. A line that
//! is not JSON is counted and skipped.

use serde_json::Value;

use crate::models::AgentSessionTool;

/// Characters a title keeps.
pub const TITLE_LIMIT: usize = 120;

/// Facts from the start of a session file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HeadFacts {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub started_at: Option<u64>,
    pub title: Option<String>,
    /// Lines that parsed as JSON objects.
    pub records: usize,
    /// Complete lines that did not.
    pub malformed: usize,
}

/// Complete lines in `window`. When the window does not reach the end of the
/// file, its last segment may be cut and is dropped.
fn lines(window: &[u8], reaches_end: bool) -> Vec<&[u8]> {
    let mut lines: Vec<&[u8]> = window.split(|byte| *byte == b'\n').collect();
    if !reaches_end || window.ends_with(b"\n") {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        .collect()
}

/// Reads the facts the inventory needs from the first bytes of a file.
pub fn read_head(tool: AgentSessionTool, window: &[u8], reaches_end: bool) -> HeadFacts {
    let mut facts = HeadFacts::default();
    for line in lines(window, reaches_end) {
        let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(line) else {
            facts.malformed += 1;
            continue;
        };
        facts.records += 1;
        let record = Value::Object(record);
        match tool {
            AgentSessionTool::Claude => claude_head(&record, &mut facts),
            AgentSessionTool::Codex => codex_head(&record, &mut facts),
        }
        if facts.session_id.is_some()
            && facts.cwd.is_some()
            && facts.started_at.is_some()
            && facts.title.is_some()
        {
            break;
        }
    }
    facts
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn fill(slot: &mut Option<String>, value: Option<String>) {
    if slot.is_none() {
        *slot = value;
    }
}

fn claude_head(record: &Value, facts: &mut HeadFacts) {
    fill(&mut facts.session_id, text(record, "sessionId"));
    fill(&mut facts.cwd, text(record, "cwd"));
    if facts.started_at.is_none() {
        facts.started_at = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp);
    }
    if facts.title.is_none() {
        facts.title = claude_prompt(record).and_then(|prompt| title_from(&redact(prompt)));
    }
}

/// The text a person typed, when `record` is such a prompt.
fn claude_prompt(record: &Value) -> Option<&str> {
    if record.get("type").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let flagged = |key: &str| record.get(key).and_then(Value::as_bool) == Some(true);
    if flagged("isMeta") || flagged("isSidechain") || flagged("isCompactSummary") {
        return None;
    }
    let message = record.get("message")?;
    if message
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|role| role != "user")
    {
        return None;
    }
    match message.get("content")? {
        Value::String(content) => typed(content),
        Value::Array(parts) => {
            if parts
                .iter()
                .any(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
            {
                return None;
            }
            parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .find_map(typed)
        }
        _ => None,
    }
}

fn codex_head(record: &Value, facts: &mut HeadFacts) {
    let kind = record.get("type").and_then(Value::as_str);
    let payload = record.get("payload");
    match (kind, payload) {
        (Some("session_meta"), Some(payload)) => {
            fill(&mut facts.session_id, text(payload, "id"));
            fill(&mut facts.cwd, text(payload, "cwd"));
            if facts.started_at.is_none() {
                facts.started_at = payload
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp);
            }
        }
        (Some("turn_context"), Some(payload)) => fill(&mut facts.cwd, text(payload, "cwd")),
        _ => {}
    }
    if facts.started_at.is_none() {
        facts.started_at = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp);
    }
    if facts.title.is_none() {
        facts.title = codex_prompt(record).and_then(|prompt| title_from(&redact(prompt)));
    }
}

fn codex_prompt(record: &Value) -> Option<&str> {
    let payload = record.get("payload")?;
    match record.get("type").and_then(Value::as_str)? {
        "response_item" => {
            if payload.get("type").and_then(Value::as_str) != Some("message")
                || payload.get("role").and_then(Value::as_str) != Some("user")
            {
                return None;
            }
            payload
                .get("content")?
                .as_array()?
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("input_text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .find_map(typed)
        }
        // Earlier Codex releases echoed prompts as events.
        "event_msg" if payload.get("type").and_then(Value::as_str) == Some("user_message") => {
            payload
                .get("message")
                .and_then(Value::as_str)
                .and_then(typed)
        }
        _ => None,
    }
}

/// `content` when it reads as something a person typed rather than text a
/// tool injected (tagged blocks, instruction files).
fn typed(content: &str) -> Option<&str> {
    let trimmed = content.trim_start();
    (!trimmed.is_empty()
        && !trimmed.starts_with('<')
        && !trimmed.starts_with("# AGENTS.md")
        && !trimmed.starts_with("Caveat:"))
    .then_some(trimmed)
}

/// A prompt with every likely secret replaced, before it becomes a title: a
/// title is shown in the window and printed by `neati sessions --json`, which
/// coding agents read (#509 review).
fn redact(prompt: &str) -> String {
    super::secrets::redact(prompt)
}

/// One line of at most [`TITLE_LIMIT`] characters, control characters removed.
pub fn title_from(prompt: &str) -> Option<String> {
    let mut title = String::new();
    let mut count = 0;
    let mut truncated = false;
    for word in prompt.split_whitespace() {
        let word: String = word.chars().filter(|c| !c.is_control()).collect();
        if word.is_empty() {
            continue;
        }
        let needed = word.chars().count() + usize::from(!title.is_empty());
        if count + needed > TITLE_LIMIT {
            let room = TITLE_LIMIT.saturating_sub(count + usize::from(!title.is_empty()));
            if room > 0 {
                if !title.is_empty() {
                    title.push(' ');
                }
                title.extend(word.chars().take(room));
            }
            truncated = true;
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(&word);
        count += needed;
    }
    if title.is_empty() {
        return None;
    }
    if truncated {
        let kept: String = title.chars().take(TITLE_LIMIT - 1).collect();
        title = format!("{}…", kept.trim_end());
    }
    Some(title)
}

/// The latest record timestamp in the last bytes of a file. `starts_mid_line`
/// says the window does not begin at the start of the file.
pub fn last_timestamp(window: &[u8], starts_mid_line: bool) -> Option<u64> {
    let mut lines = lines(window, true);
    if starts_mid_line && !lines.is_empty() {
        lines.remove(0);
    }
    lines.iter().rev().find_map(|line| {
        let record: Value = serde_json::from_slice(line).ok()?;
        record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp)
    })
}

/// Unix seconds from an RFC 3339 timestamp (`2026-11-24T10:11:12.345Z`,
/// `…+09:00`). Anything else is `None`.
pub fn parse_timestamp(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let slice = text.get(range)?;
        slice
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| slice.parse().ok())?
    };
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6 && (rest.starts_with('+') || rest.starts_with('-')) => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let hours: i64 = rest.get(1..3)?.parse().ok()?;
            let minutes: i64 = rest.get(4..6)?.parse().ok()?;
            if rest.as_bytes()[3] != b':' || hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 3600 + minutes * 60)
        }
        _ => return None,
    };
    let seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    u64::try_from(seconds).ok()
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_parse_to_unix_seconds() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_timestamp("2026-11-24T10:11:12.345Z"),
            Some(1_795_515_072)
        );
        assert_eq!(
            parse_timestamp("2026-11-24T19:11:12+09:00"),
            Some(1_795_515_072)
        );
        assert_eq!(parse_timestamp("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(parse_timestamp("2026-11-24"), None);
        assert_eq!(parse_timestamp("2026-13-24T10:11:12Z"), None);
        assert_eq!(parse_timestamp("2026-11-24T10:11:12"), None);
        assert_eq!(parse_timestamp("1969-12-31T23:59:59Z"), None);
        assert_eq!(parse_timestamp("２０２６-11-24T10:11:12Z"), None);
    }

    #[test]
    fn a_title_is_one_capped_line() {
        assert_eq!(
            title_from("  fix   the\nbuild\t"),
            Some("fix the build".into())
        );
        let long = "word ".repeat(60);
        let title = title_from(&long).expect("a title");
        assert_eq!(title.chars().count(), TITLE_LIMIT);
        assert!(title.ends_with('…'));
        let unbroken = "x".repeat(500);
        assert_eq!(
            title_from(&unbroken).expect("a title").chars().count(),
            TITLE_LIMIT
        );
        assert_eq!(title_from(" \n\t "), None);
    }

    #[test]
    fn a_cut_last_line_is_ignored_and_a_malformed_one_is_counted() {
        let window = b"{\"type\":\"user\",\"sessionId\":\"s1\",\"cwd\":\"/p\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"hello there\"}}\nnot json\n{\"type\":\"user\",\"mess";
        let facts = read_head(AgentSessionTool::Claude, window, false);
        assert_eq!(facts.session_id.as_deref(), Some("s1"));
        assert_eq!(facts.title.as_deref(), Some("hello there"));
        assert_eq!(facts.records, 1);
        assert_eq!(
            facts.malformed, 0,
            "the search stops once every fact is known"
        );

        let window = b"not json\n{\"type\":\"queue-operation\"}\n";
        let facts = read_head(AgentSessionTool::Claude, window, true);
        assert_eq!((facts.records, facts.malformed), (1, 1));
        assert_eq!(facts.title, None);
    }

    #[test]
    fn injected_and_tool_text_never_becomes_a_title() {
        let lines = [
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"output"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image"},{"type":"text","text":"the real prompt"}]}}"#,
        ]
        .join("\n");
        let facts = read_head(AgentSessionTool::Claude, lines.as_bytes(), true);
        assert_eq!(facts.title.as_deref(), Some("the real prompt"));
    }

    #[test]
    fn codex_facts_come_from_session_meta_and_the_first_typed_prompt() {
        let lines = [
            r#"{"timestamp":"2026-11-24T10:00:00.000Z","type":"session_meta","payload":{"id":"0199-abc","cwd":"/work/app","timestamp":"2026-11-24T09:59:59.000Z","base_instructions":{"text":"..."}}}"#,
            r#"{"timestamp":"2026-11-24T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"<permissions>"}]}}"#,
            r##"{"timestamp":"2026-11-24T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /work/app"},{"type":"input_text","text":"<environment_context>"}]}}"##,
            r#"{"timestamp":"2026-11-24T10:00:02.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a sessions tab"}]}}"#,
        ]
        .join("\n");
        let facts = read_head(AgentSessionTool::Codex, lines.as_bytes(), true);
        assert_eq!(facts.session_id.as_deref(), Some("0199-abc"));
        assert_eq!(facts.cwd.as_deref(), Some("/work/app"));
        assert_eq!(facts.title.as_deref(), Some("add a sessions tab"));
        assert_eq!(facts.started_at, parse_timestamp("2026-11-24T09:59:59Z"));
    }

    #[test]
    fn the_last_timestamp_skips_a_cut_first_line_and_records_without_one() {
        let window = b"mp\":\"2030-01-01T00:00:00Z\"}\n{\"timestamp\":\"2026-11-24T10:00:00Z\"}\n{\"type\":\"last-prompt\"}\n";
        assert_eq!(
            last_timestamp(window, true),
            parse_timestamp("2026-11-24T10:00:00Z")
        );
        assert_eq!(last_timestamp(b"{\"type\":\"x\"}\n", false), None);
    }
}
