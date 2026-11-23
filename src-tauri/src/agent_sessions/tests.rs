//! Fixture homes for the session inventory and bundles (#508, #509).
//!
//! Every file here is synthetic and shaped like the real formats (key names
//! and value types checked on a real machine). Nothing reads or writes the
//! real home folder.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::bundle::{
    import_bundle, read_bundle, scan_for_export, write_bundle, ExportSession, Manifest,
    ManifestFile, ManifestSession, SCHEMA,
};
use super::inventory::{
    discover, project_inventory, DiscoveredSession, Discovery, InventoryLimits,
};
use super::{SessionCancel, SessionRoots};
use crate::models::{
    AgentSessionIssue, AgentSessionTool, ProjectFolderState, SecretKind, SessionRemap,
};

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

    fn claude_folder(cwd: &Path) -> String {
        super::remap::claude_project_folder(cwd.to_str().unwrap()).unwrap()
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

fn export_of(sessions: &[DiscoveredSession]) -> Vec<ExportSession> {
    sessions
        .iter()
        .map(|session| ExportSession {
            tool: session.tool,
            session_id: session.session_id.clone(),
            original_cwd: session.cwd.clone(),
            files: session.files.clone(),
        })
        .collect()
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

fn bytes_under(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                super::relative_slash_path(root, entry.path()).unwrap(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

fn export_both(home: &Home, destination: &Path) -> Vec<ExportSession> {
    let sessions = export_of(&[
        home.session(AgentSessionTool::Claude, CLAUDE_ID),
        home.session(AgentSessionTool::Codex, CODEX_ID),
    ]);
    write_bundle(
        &sessions,
        destination,
        false,
        "9.9.9",
        &SessionCancel::new(),
    )
    .expect("the bundle is written");
    sessions
}

#[test]
fn export_then_import_round_trips_byte_for_byte() {
    let source = Home::populated();
    let out = tempfile::tempdir().unwrap();
    let bundle_path = out.path().join("s.neati-sessions");
    let sessions = export_both(&source, &bundle_path);

    let bundle = read_bundle(&bundle_path, &SessionCancel::new()).expect("the bundle verifies");
    assert_eq!(bundle.manifest.schema, SCHEMA);
    assert_eq!(bundle.manifest.neati_version, "9.9.9");
    let manifest_json = serde_json::to_value(&bundle.manifest).unwrap();
    let keys = |value: &serde_json::Value| {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&manifest_json),
        ["neati_version", "schema", "sessions"],
        "no machine or account field"
    );
    assert_eq!(
        keys(&manifest_json["sessions"][0]),
        ["files", "original_cwd", "session_id", "tool"]
    );

    let target = Home::empty();
    let report = import_bundle(
        &bundle,
        &target.roots,
        &[],
        &folder_state,
        &SessionCancel::new(),
    )
    .expect("the import succeeds");
    assert_eq!(report.written, 4);
    assert_eq!(report.sessions, 2);
    assert!(report.kept_existing.is_empty());
    assert_eq!(report.rewritten, 0);

    for session in &sessions {
        for file in &session.files {
            let imported = target.roots.tool_root(session.tool).join(&file.relative);
            assert_eq!(
                std::fs::read(&imported).expect("imported file"),
                std::fs::read(&file.path).expect("source file"),
                "{} is byte for byte",
                file.relative
            );
        }
    }
    // The imported sessions appear in the target's own inventory.
    let imported = target.discover();
    assert_eq!(imported.sessions.len(), 2);
}

#[test]
fn import_keeps_an_existing_file() {
    let source = Home::populated();
    let out = tempfile::tempdir().unwrap();
    let bundle_path = out.path().join("s.neati-sessions");
    export_both(&source, &bundle_path);
    let bundle = read_bundle(&bundle_path, &SessionCancel::new()).unwrap();

    let target = Home::empty();
    let existing = target.roots.codex_home.join(format!(
        "sessions/2026/11/21/rollout-2026-11-21T10-00-00-{CODEX_ID}.jsonl"
    ));
    write(&existing, b"the person's own copy\n");
    let report = import_bundle(
        &bundle,
        &target.roots,
        &[],
        &folder_state,
        &SessionCancel::new(),
    )
    .expect("the import succeeds");
    assert_eq!(
        std::fs::read(&existing).unwrap(),
        b"the person's own copy\n"
    );
    assert_eq!(report.kept_existing.len(), 1);
    assert!(report.kept_existing[0].starts_with("codex/sessions/"));
    assert_eq!(report.written, 3);

    // A second import writes nothing new and replaces nothing.
    let before = bytes_under(&target.root);
    let again = import_bundle(
        &bundle,
        &target.roots,
        &[],
        &folder_state,
        &SessionCancel::new(),
    )
    .expect("the second import succeeds");
    assert_eq!(again.written, 0);
    assert_eq!(again.kept_existing.len(), 4);
    assert_eq!(bytes_under(&target.root), before);
}

#[test]
fn export_never_replaces_a_file() {
    let source = Home::populated();
    let out = tempfile::tempdir().unwrap();
    let bundle_path = out.path().join("s.neati-sessions");
    write(&bundle_path, b"something else");
    let sessions = export_of(&[source.session(AgentSessionTool::Codex, CODEX_ID)]);
    let error = write_bundle(&sessions, &bundle_path, false, "1", &SessionCancel::new())
        .expect_err("an existing destination is refused");
    assert!(error.contains("never replaces"));
    assert_eq!(std::fs::read(&bundle_path).unwrap(), b"something else");
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        1,
        "no partial file is left"
    );
}

#[test]
fn secret_findings_are_counts_and_refuse_without_permission() {
    let home = Home::empty();
    let present = home.present.to_str().unwrap().to_string();
    let claude = home
        .roots
        .claude_projects
        .join(Home::claude_folder(&home.present));
    let key = super::secrets::fake_token("sk-ant-", 32);
    write(
        &claude.join(format!("{CLAUDE_ID}.jsonl")),
        claude_session(
            CLAUDE_ID,
            &present,
            "deploy it",
            &format!(
                "export ANTHROPIC_API_KEY={key} {}",
                super::secrets::fake_token("ghp_", 36)
            ),
        )
        .as_bytes(),
    );
    let sessions = export_of(&[home.session(AgentSessionTool::Claude, CLAUDE_ID)]);
    let scan = scan_for_export(&sessions, &SessionCancel::new()).unwrap();
    assert_eq!(scan.secrets.count(SecretKind::AnthropicKey), 1);
    assert_eq!(scan.secrets.count(SecretKind::GithubToken), 1);
    let findings = serde_json::to_string(&scan.secrets.findings()).unwrap();
    assert!(!findings.contains(&key));

    let out = tempfile::tempdir().unwrap();
    let refused = out.path().join("refused.neati-sessions");
    let error = write_bundle(&sessions, &refused, false, "1", &SessionCancel::new())
        .expect_err("findings refuse the bundle");
    assert!(!error.contains(&key));
    assert!(!refused.exists());
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        0,
        "no partial file is left"
    );

    let allowed = out.path().join("allowed.neati-sessions");
    let report = write_bundle(&sessions, &allowed, true, "1", &SessionCancel::new())
        .expect("allowed findings write the bundle");
    assert_eq!(report.secrets.count(SecretKind::AnthropicKey), 1);
    assert!(allowed.is_file());
}

/// Writes a zip with the given entries, for hostile bundles.
fn raw_bundle(path: &Path, manifest: &Manifest, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(manifest).unwrap())
        .unwrap();
    zip.finish().unwrap();
}

fn one_file_manifest(tool: AgentSessionTool, id: &str, path: &str, bytes: &[u8]) -> Manifest {
    Manifest {
        schema: SCHEMA.to_string(),
        neati_version: "1".to_string(),
        sessions: vec![ManifestSession {
            tool,
            session_id: id.to_string(),
            original_cwd: Some("/somewhere".to_string()),
            files: vec![ManifestFile {
                path: path.to_string(),
                size: bytes.len() as u64,
                sha256: crate::hash::sha256_hex(bytes),
            }],
        }],
    }
}

#[test]
fn a_tampered_checksum_is_refused_before_anything_is_written() {
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("t.neati-sessions");
    let original = b"{\"type\":\"user\"}\n";
    let manifest = one_file_manifest(
        AgentSessionTool::Claude,
        CLAUDE_ID,
        &format!("-p/{CLAUDE_ID}.jsonl"),
        original,
    );
    let tampered = b"{\"type\":\"evil\"}\n";
    raw_bundle(
        &path,
        &manifest,
        &[(&format!("claude/-p/{CLAUDE_ID}.jsonl"), tampered)],
    );
    let error = read_bundle(&path, &SessionCancel::new()).expect_err("a changed file is refused");
    assert!(error.contains("checksum"), "{error}");

    // A size that disagrees with the manifest is refused the same way.
    let longer = b"{\"type\":\"user\"}\n\n";
    raw_bundle(
        &path,
        &manifest,
        &[(&format!("claude/-p/{CLAUDE_ID}.jsonl"), longer)],
    );
    assert!(read_bundle(&path, &SessionCancel::new()).is_err());
}

#[test]
fn traversal_absolute_paths_and_unlisted_entries_are_refused() {
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("t.neati-sessions");
    let bytes = b"{}\n";
    for hostile in [
        format!("../{CLAUDE_ID}.jsonl"),
        format!("-p/../../{CLAUDE_ID}.jsonl"),
        format!("/etc/{CLAUDE_ID}.jsonl"),
        format!("-p\\..\\{CLAUDE_ID}.jsonl"),
        format!("./-p/{CLAUDE_ID}.jsonl"),
        format!("C:/-p/{CLAUDE_ID}.jsonl"),
        "-p/other-session.jsonl".to_string(),
    ] {
        let manifest = one_file_manifest(AgentSessionTool::Claude, CLAUDE_ID, &hostile, bytes);
        raw_bundle(&path, &manifest, &[(&format!("claude/{hostile}"), bytes)]);
        let error =
            read_bundle(&path, &SessionCancel::new()).expect_err("a hostile path is refused");
        assert!(error.contains("unsafe path"), "{hostile}: {error}");
    }
    let codex = one_file_manifest(AgentSessionTool::Codex, CODEX_ID, "config.toml", bytes);
    raw_bundle(&path, &codex, &[("codex/config.toml", bytes)]);
    assert!(
        read_bundle(&path, &SessionCancel::new()).is_err(),
        "only session folders are written"
    );

    let good_path = format!("-p/{CLAUDE_ID}.jsonl");
    let manifest = one_file_manifest(AgentSessionTool::Claude, CLAUDE_ID, &good_path, bytes);
    raw_bundle(
        &path,
        &manifest,
        &[
            (&format!("claude/{good_path}"), bytes),
            ("claude/-p/extra.jsonl", bytes),
        ],
    );
    let error =
        read_bundle(&path, &SessionCancel::new()).expect_err("an unlisted entry is refused");
    assert!(error.contains("does not list"), "{error}");

    let mut other_schema = manifest.clone();
    other_schema.schema = "neati.sessions/2".to_string();
    raw_bundle(
        &path,
        &other_schema,
        &[(&format!("claude/{good_path}"), bytes)],
    );
    assert!(read_bundle(&path, &SessionCancel::new()).is_err());

    write(&path, b"not a zip");
    assert!(read_bundle(&path, &SessionCancel::new()).is_err());
}

#[test]
fn a_link_entry_is_refused() {
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("t.neati-sessions");
    let good_path = format!("-p/{CLAUDE_ID}.jsonl");
    let manifest = one_file_manifest(
        AgentSessionTool::Claude,
        CLAUDE_ID,
        &good_path,
        b"/etc/passwd",
    );
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.add_symlink(
        format!("claude/{good_path}"),
        "/etc/passwd",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    zip.finish().unwrap();
    let error = read_bundle(&path, &SessionCancel::new()).expect_err("a link entry is refused");
    assert!(error.contains("link"), "{error}");
}

#[cfg(unix)]
#[test]
fn import_never_writes_through_a_linked_session_folder() {
    let source = Home::populated();
    let out = tempfile::tempdir().unwrap();
    let bundle_path = out.path().join("s.neati-sessions");
    export_both(&source, &bundle_path);
    let bundle = read_bundle(&bundle_path, &SessionCancel::new()).unwrap();

    let target = Home::empty();
    let elsewhere = target.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::create_dir_all(target.roots.claude_projects.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &target.roots.claude_projects).unwrap();
    let error = import_bundle(
        &bundle,
        &target.roots,
        &[],
        &folder_state,
        &SessionCancel::new(),
    )
    .expect_err("a linked session folder is refused");
    assert!(error.contains("link"), "{error}");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn a_remap_moves_claude_sessions_and_rewrites_only_cwd_fields() {
    let source = Home::empty();
    let old = source.missing.to_str().unwrap().to_string();
    let claude = source
        .roots
        .claude_projects
        .join(Home::claude_folder(&source.missing));
    let main = claude_session(CLAUDE_ID, &old, &format!("Look at {old}/README"), "");
    write(&claude.join(format!("{CLAUDE_ID}.jsonl")), main.as_bytes());
    write(
        &claude
            .join(CLAUDE_ID)
            .join("subagents")
            .join("agent-a1.jsonl"),
        claude_session("agent-a1", &format!("{old}/sub"), "sub task", "").as_bytes(),
    );
    write(
        &claude.join(CLAUDE_ID).join("tool-results").join("r1.txt"),
        old.as_bytes(),
    );
    let out = tempfile::tempdir().unwrap();
    let bundle_path = out.path().join("s.neati-sessions");
    let sessions = export_of(&[source.session(AgentSessionTool::Claude, CLAUDE_ID)]);
    write_bundle(&sessions, &bundle_path, false, "1", &SessionCancel::new()).unwrap();
    let bundle = read_bundle(&bundle_path, &SessionCancel::new()).unwrap();

    let target = Home::empty();
    let new = target.present.to_str().unwrap().to_string();
    // The old folder must be missing here and the new one present.
    let refused = import_bundle(
        &bundle,
        &target.roots,
        &[SessionRemap {
            from: old.clone(),
            to: target.root.join("nope").to_string_lossy().into_owned(),
        }],
        &folder_state,
        &SessionCancel::new(),
    );
    assert!(refused.is_err());
    assert!(import_bundle(
        &bundle,
        &target.roots,
        &[SessionRemap {
            from: "/never/recorded".into(),
            to: new.clone()
        }],
        &folder_state,
        &SessionCancel::new(),
    )
    .is_err());
    assert!(
        !target.roots.claude_projects.exists(),
        "a refused remap writes nothing"
    );

    let report = import_bundle(
        &bundle,
        &target.roots,
        &[SessionRemap {
            from: format!("{old}/"),
            to: new.clone(),
        }],
        &folder_state,
        &SessionCancel::new(),
    )
    .expect("the remapped import succeeds");
    assert_eq!(report.written, 3);
    assert_eq!(report.rewritten, 2, "both JSONL files had cwd fields");

    let folder = target
        .roots
        .claude_projects
        .join(Home::claude_folder(&target.present));
    let imported = std::fs::read_to_string(folder.join(format!("{CLAUDE_ID}.jsonl"))).unwrap();
    assert_eq!(
        imported,
        main.replace(&format!("\"cwd\":\"{old}\""), &format!("\"cwd\":\"{new}\"")),
        "only cwd fields change; the prompt that names the old folder stays"
    );
    assert!(imported.contains(&format!("Look at {old}/README")));
    let sub =
        std::fs::read_to_string(folder.join(CLAUDE_ID).join("subagents/agent-a1.jsonl")).unwrap();
    assert!(sub.contains(&format!("\"cwd\":\"{new}/sub\"")));
    assert_eq!(
        std::fs::read_to_string(folder.join(CLAUDE_ID).join("tool-results/r1.txt")).unwrap(),
        old,
        "non-JSONL files are copied as they were"
    );
    // The moved session is listed under its new project.
    let inventory = project_inventory(&target.discover(), 1, &folder_state);
    assert_eq!(inventory.projects.len(), 1);
    assert_eq!(inventory.projects[0].cwd.as_deref(), Some(new.as_str()));
    assert_eq!(inventory.projects[0].folder, ProjectFolderState::Present);
}

#[test]
fn a_stopped_export_leaves_no_file() {
    let home = Home::populated();
    let sessions = export_of(&[home.session(AgentSessionTool::Codex, CODEX_ID)]);
    let out = tempfile::tempdir().unwrap();
    let cancel = SessionCancel::new();
    cancel.cancel();
    assert!(write_bundle(
        &sessions,
        &out.path().join("x.neati-sessions"),
        false,
        "1",
        &cancel
    )
    .is_err());
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn recent_working_directories_count_sessions_per_folder() {
    let home = Home::populated();
    let environment =
        neati_platform::PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home(&home.root);
    let folders = super::recent_working_directories(&environment);
    // Both the present and the missing project are named; existence is the
    // suggestion's concern, not this reader's.
    assert!(folders
        .iter()
        .any(|(path, count)| path == &home.present && *count >= 1));
    assert!(folders.iter().any(|(path, _)| path == &home.missing));
    assert!(folders.iter().all(|(path, _)| path.is_absolute()));

    let empty = Home::empty();
    let environment =
        neati_platform::PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home(&empty.root);
    assert!(super::recent_working_directories(&environment).is_empty());
}
