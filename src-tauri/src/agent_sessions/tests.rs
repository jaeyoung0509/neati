//! Fixture homes for the session inventory (#508).
//!
//! Every file here is synthetic and shaped like the real formats (key names
//! and value types checked on a real machine). Nothing reads or writes the
//! real home folder.

use std::path::{Path, PathBuf};

use super::inventory::{
    discover, project_inventory, DiscoveredSession, Discovery, InventoryLimits,
};
use super::{SessionCancel, SessionRoots};
use crate::models::{AgentSessionIssue, AgentSessionTool, ProjectFolderState};

const BODY: &str = "TRANSCRIPT_BODY_MARKER";
const QUEUED: &str = "QUEUED_PROMPT_MARKER";
const CLAUDE_ID: &str = "0b5c7e2a-1111-4c3e-9a1b-2c3d4e5f6a7b";
const CODEX_ID: &str = "019a0000-2222-7abc-8def-0123456789ab";

fn write(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("parent folders");
    std::fs::write(path, contents).expect("fixture file");
}

fn claude_session(id: &str, cwd: &str, prompt: &str, extra: &str) -> String {
    [
        format!(r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-20T09:00:00.000Z","sessionId":"{id}","content":"{QUEUED}"}}"#),
        format!(r#"{{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"{cwd}","sessionId":"{id}","version":"2.1.0","gitBranch":"main","type":"user","isMeta":true,"message":{{"role":"user","content":"<local-command-caveat>{BODY}</local-command-caveat>"}},"uuid":"u0","timestamp":"2026-11-20T09:00:00.500Z"}}"#),
        format!(r#"{{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"{cwd}","sessionId":"{id}","version":"2.1.0","gitBranch":"main","type":"user","message":{{"role":"user","content":"{prompt}"}},"uuid":"u1","timestamp":"2026-11-20T09:00:01.000Z"}}"#),
        format!(r#"{{"parentUuid":"u1","isSidechain":false,"cwd":"{cwd}","sessionId":"{id}","type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{BODY} {extra}"}}]}},"uuid":"a1","timestamp":"2026-11-20T09:05:00.000Z"}}"#),
        format!(r#"{{"type":"last-prompt","lastPrompt":"{BODY}","sessionId":"{id}"}}"#),
    ]
    .join("\n")
        + "\n"
}

fn codex_session(id: &str, cwd: &str, prompt: &str) -> String {
    [
        format!(r#"{{"timestamp":"2026-11-21T10:00:00.000Z","type":"session_meta","payload":{{"id":"{id}","session_id":"{id}","timestamp":"2026-11-21T10:00:00.000Z","cwd":"{cwd}","originator":"codex_cli_rs","cli_version":"0.99.0","source":"cli","base_instructions":{{"text":"{BODY}"}}}}}}"#),
        format!(r#"{{"timestamp":"2026-11-21T10:00:01.000Z","type":"response_item","payload":{{"type":"message","role":"developer","content":[{{"type":"input_text","text":"<permissions instructions>{BODY}"}}]}}}}"#),
        format!(r##"{{"timestamp":"2026-11-21T10:00:01.000Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"# AGENTS.md instructions for {cwd}"}},{{"type":"input_text","text":"<environment_context>{BODY}</environment_context>"}}]}}}}"##),
        format!(r#"{{"timestamp":"2026-11-21T10:00:02.000Z","type":"turn_context","payload":{{"cwd":"{cwd}","model":"m"}}}}"#),
        format!(r#"{{"timestamp":"2026-11-21T10:00:03.000Z","type":"response_item","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"{prompt}"}}]}}}}"#),
        r#"{"timestamp":"2026-11-21T11:30:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#.to_string(),
    ]
    .join("\n")
        + "\n"
}

struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    roots: SessionRoots,
    /// A project folder that exists.
    present: PathBuf,
    /// A project folder that does not.
    missing: PathBuf,
}

impl Home {
    fn empty() -> Self {
        let dir = tempfile::tempdir().expect("temporary home");
        let root = dir.path().canonicalize().expect("canonical home");
        let present = root.join("code").join("app");
        std::fs::create_dir_all(&present).expect("project folder");
        let missing = root.join("code").join("gone_project.v1");
        Self {
            roots: SessionRoots::from_home(&root),
            _dir: dir,
            root,
            present,
            missing,
        }
    }

    /// Claude Code's folder name for a project: every character other than
    /// an ASCII letter or digit becomes `-`.
    fn claude_folder(cwd: &Path) -> String {
        cwd.to_str()
            .unwrap()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    }

    /// One Claude session in the present project (with side files), one
    /// Codex session in the missing project, and malformed, partial and linked
    /// files that must not break the inventory.
    fn populated() -> Self {
        let home = Self::empty();
        let present = home.present.to_str().unwrap().to_string();
        let missing = home.missing.to_str().unwrap().to_string();
        let claude = home
            .roots
            .claude_projects
            .join(Self::claude_folder(&home.present));
        write(
            &claude.join(format!("{CLAUDE_ID}.jsonl")),
            claude_session(CLAUDE_ID, &present, "Fix the flaky parser test", "").as_bytes(),
        );
        write(
            &claude
                .join(CLAUDE_ID)
                .join("subagents")
                .join("agent-a1.jsonl"),
            claude_session("agent-a1", &format!("{present}/sub"), "sub task", "").as_bytes(),
        );
        write(
            &claude.join(CLAUDE_ID).join("tool-results").join("r1.txt"),
            b"tool output\n",
        );
        write(
            &claude.join("memory").join("note.md"),
            b"project memory, not a session\n",
        );
        // Garbage, and a file cut in the middle of its last record.
        write(
            &claude.join("deadbeef-0000-4000-8000-000000000001.jsonl"),
            b"\x00\x01 not json at all\n",
        );
        let partial = claude_session(
            "deadbeef-0000-4000-8000-000000000002",
            &present,
            "Partial prompt",
            "",
        );
        write(
            &claude.join("deadbeef-0000-4000-8000-000000000002.jsonl"),
            &partial.as_bytes()[..partial.len() - 30],
        );
        let codex = home.roots.codex_home.join("sessions/2026/11/21");
        write(
            &codex.join(format!("rollout-2026-11-21T10-00-00-{CODEX_ID}.jsonl")),
            codex_session(CODEX_ID, &missing, "Add the sessions tab").as_bytes(),
        );
        #[cfg(unix)]
        {
            let outside = home.root.join("outside.jsonl");
            write(
                &outside,
                claude_session("linked", &present, "linked", "").as_bytes(),
            );
            std::os::unix::fs::symlink(&outside, claude.join("linked.jsonl")).unwrap();
            let elsewhere = home.root.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            write(&elsewhere.join("x.jsonl"), b"{}\n");
            std::os::unix::fs::symlink(
                &elsewhere,
                home.roots.claude_projects.join("-linked-project"),
            )
            .unwrap();
        }
        home
    }

    fn discover(&self) -> Discovery {
        discover(
            &self.roots,
            &InventoryLimits::default(),
            &SessionCancel::new(),
        )
    }

    fn session(&self, tool: AgentSessionTool, id: &str) -> DiscoveredSession {
        self.discover()
            .sessions
            .into_iter()
            .find(|session| session.tool == tool && session.session_id == id)
            .expect("the fixture session")
    }
}

fn folder_state(cwd: &str) -> ProjectFolderState {
    if Path::new(cwd).is_dir() {
        ProjectFolderState::Present
    } else {
        ProjectFolderState::Missing
    }
}

#[test]
fn the_inventory_groups_by_project_and_flags_orphans() {
    let home = Home::populated();
    let discovery = home.discover();
    let inventory = project_inventory(&discovery, 1, &folder_state);

    let claude = home.session(AgentSessionTool::Claude, CLAUDE_ID);
    assert_eq!(claude.title.as_deref(), Some("Fix the flaky parser test"));
    assert_eq!(claude.cwd.as_deref(), home.present.to_str());
    assert_eq!(
        claude.files.len(),
        3,
        "the session file and its two side files"
    );
    assert!(claude.dates_from_records);
    assert_eq!(
        claude.last_activity_at,
        super::records::parse_timestamp("2026-11-20T09:05:00Z")
    );

    let codex = home.session(AgentSessionTool::Codex, CODEX_ID);
    assert_eq!(codex.title.as_deref(), Some("Add the sessions tab"));
    assert_eq!(
        codex.last_activity_at,
        super::records::parse_timestamp("2026-11-21T11:30:00Z")
    );

    let orphan = inventory
        .projects
        .iter()
        .find(|project| project.cwd.as_deref() == home.missing.to_str())
        .expect("the missing project is listed");
    assert_eq!(orphan.folder, ProjectFolderState::Missing);
    assert_eq!(orphan.display_name, "gone_project.v1");
    let present = inventory
        .projects
        .iter()
        .find(|project| project.cwd.as_deref() == home.present.to_str())
        .expect("the present project is listed");
    assert_eq!(present.folder, ProjectFolderState::Present);
    assert_eq!(
        present.sessions.len(),
        2,
        "the parser session and the partial one"
    );
    let position = |cwd: &Path| {
        inventory
            .projects
            .iter()
            .position(|project| project.cwd.as_deref() == cwd.to_str())
            .unwrap()
    };
    assert!(
        position(&home.missing) < position(&home.present),
        "the more recent project comes first"
    );

    let unreadable = discovery
        .sessions
        .iter()
        .find(|session| session.session_id == "deadbeef-0000-4000-8000-000000000001")
        .expect("the malformed file is still listed");
    assert_eq!(unreadable.issue, Some(AgentSessionIssue::NoRecords));
    let partial = discovery
        .sessions
        .iter()
        .find(|session| session.session_id == "deadbeef-0000-4000-8000-000000000002")
        .expect("the partial file is listed");
    assert_eq!(partial.title.as_deref(), Some("Partial prompt"));
    assert_eq!(partial.issue, None);

    assert!(
        discovery
            .sessions
            .iter()
            .all(|session| session.session_id != "linked"
                && !session
                    .files
                    .iter()
                    .any(|file| file.relative.starts_with("-linked-project"))),
        "links are never followed"
    );
    let claude_total = inventory
        .tools
        .iter()
        .find(|tool| tool.tool == AgentSessionTool::Claude)
        .unwrap();
    assert!(claude_total.present);
    assert_eq!(claude_total.sessions, 3);
    assert_eq!(claude_total.files, 5);
    let codex_total = inventory
        .tools
        .iter()
        .find(|tool| tool.tool == AgentSessionTool::Codex)
        .unwrap();
    assert_eq!((codex_total.sessions, codex_total.files), (1, 1));
    assert!(inventory
        .incomplete_reasons
        .iter()
        .any(|reason| reason.starts_with("Claude Code: 1 session could not be read")));
}

#[test]
fn no_transcript_text_but_the_capped_title_crosses_ipc() {
    let home = Home::populated();
    let inventory = project_inventory(&home.discover(), 1, &folder_state);
    let json = serde_json::to_string(&inventory).expect("the inventory serializes");
    assert!(json.contains("Fix the flaky parser test"));
    for marker in [BODY, QUEUED, "tool output", "project memory", "sub task"] {
        assert!(!json.contains(marker), "{marker} crossed IPC");
    }
    assert!(
        !json.contains(".jsonl"),
        "session file paths stay in the backend"
    );
}

#[test]
fn caps_and_the_read_budget_bound_the_inventory() {
    let home = Home::populated();
    let limits = InventoryLimits {
        max_sessions_per_tool: 1,
        ..InventoryLimits::default()
    };
    let discovery = discover(&home.roots, &limits, &SessionCancel::new());
    assert!(discovery.truncated);
    assert_eq!(
        discovery
            .sessions
            .iter()
            .filter(|session| session.tool == AgentSessionTool::Claude)
            .count(),
        1
    );
    assert!(discovery
        .reasons
        .iter()
        .any(|reason| reason.contains("limit 1")));

    let starved = InventoryLimits {
        read_budget: 0,
        ..InventoryLimits::default()
    };
    let discovery = discover(&home.roots, &starved, &SessionCancel::new());
    assert!(discovery.truncated);
    assert!(discovery.sessions.iter().all(|session| session.issue
        == Some(AgentSessionIssue::NotRead)
        && session.title.is_none()));
}

#[test]
fn a_stopped_inventory_reads_nothing_more() {
    let home = Home::populated();
    let cancel = SessionCancel::new();
    cancel.cancel();
    let discovery = discover(&home.roots, &InventoryLimits::default(), &cancel);
    assert!(discovery.cancelled);
    assert!(discovery.sessions.is_empty());
}

#[test]
fn a_home_without_session_folders_is_empty_not_an_error() {
    let home = Home::empty();
    let inventory = project_inventory(&home.discover(), 1, &folder_state);
    assert!(inventory.projects.is_empty());
    assert!(inventory
        .tools
        .iter()
        .all(|tool| !tool.present && tool.sessions == 0));
}
