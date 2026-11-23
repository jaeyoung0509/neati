//! The ports-and-adapters boundary of #498, as a build invariant.
//!
//! The CLI is a driving adapter: its handlers may reach only the driving ports
//! `composition::application_ports` builds. The one-button use case is the
//! application core: only its `adapters.rs` may name a concrete service, and
//! none of it may name Tauri, a terminal, or stdout. A reviewer would catch a
//! slip once; this catches it every time.

use std::fs;
use std::path::{Path, PathBuf};

fn source(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// Every non-test `.rs` file directly in `directory`, except `allowed`.
fn files(directory: &str, allowed: &[&str]) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = fs::read_dir(source(directory))
        .expect("source directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            (name != "tests.rs" && !allowed.contains(&name.as_str()))
                .then(|| (name, fs::read_to_string(&path).expect("readable source")))
        })
        .collect();
    found.sort();
    found
}

/// Lines of `text` that are code, not comments.
fn code(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with("//"))
}

fn offenders(files: &[(String, String)], forbidden: &[&str]) -> Vec<String> {
    let mut offenders = Vec::new();
    for (name, text) in files {
        for line in code(text) {
            for needle in forbidden {
                if line.contains(needle) {
                    offenders.push(format!("{name}: `{needle}` in `{}`", line.trim()));
                }
            }
        }
    }
    offenders
}

const CONCRETE_SERVICES: &[&str] = &[
    "DesktopState",
    "CleanupService",
    "StorageService",
    "SystemService",
    "AiService",
    ".state.",
];

#[test]
fn cli_handlers_reach_only_the_driving_ports() {
    // `context.rs` is the CLI's composition root: it may build the state it
    // hands to `application_ports`, and nothing else may.
    let handlers = files("src/cli", &["context.rs"]);
    assert!(handlers.len() >= 6, "the CLI handler modules were found");
    let found = offenders(&handlers, CONCRETE_SERVICES);
    assert!(
        found.is_empty(),
        "CLI handlers bypass the ports:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_one_button_core_names_no_service_and_no_interface() {
    let core = files("src/services/one_button_clean", &["adapters.rs"]);
    assert_eq!(
        core.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["mod.rs", "ports.rs", "selection.rs"]
    );
    let mut forbidden = CONCRETE_SERVICES.to_vec();
    forbidden.extend(["tauri", "println!", "eprintln!", "std::io::std"]);
    let found = offenders(&core, &forbidden);
    assert!(
        found.is_empty(),
        "the use case core reaches outward:\n{}",
        found.join("\n")
    );
}

#[test]
fn ports_are_bound_only_in_the_composition_root() {
    let mut bindings = Vec::new();
    for directory in ["src/cli", "src/services", "src/commands"] {
        for (name, text) in files(directory, &[]) {
            if code(&text).any(|line| {
                line.contains("ServiceCacheCleanup::new")
                    || line.contains("ServiceProjectCleanup::new")
            }) {
                bindings.push(format!("{directory}/{name}"));
            }
        }
    }
    assert!(
        bindings.is_empty(),
        "ports bound outside composition: {bindings:?}"
    );
    let composition =
        fs::read_to_string(source("src/composition/interfaces.rs")).expect("composition source");
    assert!(composition.contains("ServiceCacheCleanup::new"));
    assert!(composition.contains("ServiceProjectCleanup::new"));
}
