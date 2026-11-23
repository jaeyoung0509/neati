//! Moving a session to another project folder on import (#509). Pure.
//!
//! A remap names an old folder and a new one. Every `cwd` field whose value is
//! the old folder, or a path inside it, is rewritten; nothing else in a line
//! changes. A line that is not a JSON object is copied as it was.
//!
//! Claude Code names a project's session folder after the project path with
//! every character other than an ASCII letter or digit replaced by `-`
//! (`/Users/me/app` → `-Users-me-app`). That rule was checked against the
//! session folders on a real machine without reading their contents. A
//! remapped Claude session goes to the folder named for its new path.

use serde_json::Value;

/// Claude Code's longest plain folder name; longer paths are shortened by a
/// rule neati has not verified, so a remap to one is refused.
pub const CLAUDE_FOLDER_LIMIT: usize = 200;

/// The `~/.claude/projects` folder name for a project path.
pub fn claude_project_folder(cwd: &str) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let folder: String = cwd
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    (folder.len() <= CLAUDE_FOLDER_LIMIT).then_some(folder)
}

/// `value` with `from` replaced by `to`, when `value` is `from` or inside it.
pub fn remap_path(value: &str, from: &str, to: &str) -> Option<String> {
    let from = from.trim_end_matches('/');
    let to = to.trim_end_matches('/');
    if from.is_empty() {
        return None;
    }
    if value == from {
        return Some(to.to_string());
    }
    value
        .strip_prefix(from)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{to}{rest}"))
}

/// Rewrites the `cwd` fields of one JSONL line (without its line ending).
/// `None` means the line stays exactly as it was.
pub fn rewrite_line(line: &[u8], from: &str, to: &str) -> Option<Vec<u8>> {
    let original: Value = serde_json::from_slice(line).ok()?;
    if !original.is_object() {
        return None;
    }
    let mut expected = original.clone();
    let mut changes = Vec::new();
    remap_cwd_fields(&mut expected, from, to, &mut changes);
    if changes.is_empty() {
        return None;
    }
    // Replace the serialized field in place, so the rest of the line keeps
    // its exact bytes; fall back to re-serializing when the writer spaced it
    // differently.
    let mut text = line.to_vec();
    changes.sort();
    changes.dedup();
    for (old, new) in &changes {
        let needle = field_bytes(old);
        let replacement = field_bytes(new);
        text = replace_all(&text, &needle, &replacement);
    }
    match serde_json::from_slice::<Value>(&text) {
        Ok(rewritten) if rewritten == expected => Some(text),
        _ => serde_json::to_vec(&expected).ok(),
    }
}

fn field_bytes(value: &str) -> Vec<u8> {
    let mut bytes = b"\"cwd\":".to_vec();
    bytes.extend(serde_json::to_vec(value).unwrap_or_default());
    bytes
}

fn replace_all(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut index = 0;
    while index < haystack.len() {
        if haystack[index..].starts_with(needle) {
            out.extend_from_slice(replacement);
            index += needle.len();
        } else {
            out.push(haystack[index]);
            index += 1;
        }
    }
    out
}

fn remap_cwd_fields(value: &mut Value, from: &str, to: &str, changes: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if key == "cwd" {
                    if let Value::String(current) = entry {
                        if let Some(next) = remap_path(current, from, to) {
                            changes.push((current.clone(), next.clone()));
                            *current = next;
                        }
                        continue;
                    }
                }
                remap_cwd_fields(entry, from, to, changes);
            }
        }
        Value::Array(items) => {
            for item in items {
                remap_cwd_fields(item, from, to, changes);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_folders_replace_every_non_alphanumeric_character() {
        assert_eq!(
            claude_project_folder("/Users/me/my_app.v2").as_deref(),
            Some("-Users-me-my-app-v2")
        );
        assert_eq!(claude_project_folder(""), None);
        assert_eq!(
            claude_project_folder(&format!("/{}", "a".repeat(250))),
            None
        );
    }

    #[test]
    fn a_path_is_remapped_only_on_a_component_boundary() {
        assert_eq!(
            remap_path("/old/app", "/old/app", "/new/app").as_deref(),
            Some("/new/app")
        );
        assert_eq!(
            remap_path("/old/app/src", "/old/app/", "/new/app").as_deref(),
            Some("/new/app/src")
        );
        assert_eq!(remap_path("/old/application", "/old/app", "/new/app"), None);
        assert_eq!(remap_path("/elsewhere", "/old/app", "/new/app"), None);
    }

    #[test]
    fn only_cwd_fields_change_and_the_rest_keeps_its_bytes() {
        let line = br#"{"type":"user","cwd":"/old/app","note":"/old/app stays","n":1.50,"nested":{"cwd":"/old/app/sub"},"list":[{"cwd":"/other"}]}"#;
        let rewritten = rewrite_line(line, "/old/app", "/new/place").expect("rewritten");
        assert_eq!(
            String::from_utf8(rewritten).unwrap(),
            r#"{"type":"user","cwd":"/new/place","note":"/old/app stays","n":1.50,"nested":{"cwd":"/new/place/sub"},"list":[{"cwd":"/other"}]}"#
        );
    }

    #[test]
    fn untouched_and_malformed_lines_are_left_alone() {
        assert_eq!(rewrite_line(br#"{"cwd":"/other"}"#, "/old", "/new"), None);
        assert_eq!(rewrite_line(b"{\"cwd\":\"/old\"", "/old", "/new"), None);
        assert_eq!(rewrite_line(b"[1,2]", "/old", "/new"), None);
    }

    #[test]
    fn a_differently_spaced_field_is_still_rewritten() {
        let line = br#"{"cwd" : "/old/app", "type":"x"}"#;
        let rewritten = rewrite_line(line, "/old/app", "/new").expect("rewritten");
        let value: Value = serde_json::from_slice(&rewritten).unwrap();
        assert_eq!(value["cwd"], "/new");
        assert_eq!(value["type"], "x");
    }
}
