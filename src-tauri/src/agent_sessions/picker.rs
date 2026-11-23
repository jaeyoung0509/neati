//! Native pickers for session bundles (#509).
//!
//! The window never submits a path for a bundle: the person picks it in the
//! system dialog the backend opens, as for developer workspaces and Keep Awake
//! applications. The AppleScript is static; nothing is interpolated into it.
//! Cancelling a dialog is an empty answer, not an error.

use std::path::PathBuf;

/// What the window's bundle workflows ask the system for.
pub trait BundlePicker: Send + Sync {
    /// Where a new bundle should be written.
    fn export_destination(&self) -> Result<Option<PathBuf>, String>;
    /// An existing bundle to import.
    fn bundle_to_import(&self) -> Result<Option<PathBuf>, String>;
    /// A project folder on this Mac, for a remap.
    fn project_folder(&self) -> Result<Option<PathBuf>, String>;
}

/// The system dialogs.
pub struct NativeBundlePicker;

#[cfg(target_os = "macos")]
fn run_dialog(script: &'static str) -> Result<Option<PathBuf>, String> {
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|error| format!("Could not open the file dialog: {error}"))?;
    if !output.status.success() {
        return Err(crate::diagnostics::sanitize_log(
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}

#[cfg(not(target_os = "macos"))]
fn run_dialog(_script: &'static str) -> Result<Option<PathBuf>, String> {
    Err("Session bundles are picked with the macOS file dialog; use `neati sessions export` or `import` here.".to_string())
}

impl BundlePicker for NativeBundlePicker {
    fn export_destination(&self) -> Result<Option<PathBuf>, String> {
        run_dialog(
            r#"
            try
                set chosenFile to choose file name with prompt "Save the session bundle" default name "agent-sessions.neati-sessions"
                return POSIX path of chosenFile
            on error number -128
                return ""
            end try
        "#,
        )
        .map(|path| {
            path.map(|path| {
                if path.extension().and_then(|extension| extension.to_str())
                    == Some(super::bundle::EXTENSION)
                {
                    path
                } else {
                    let mut name = path.as_os_str().to_os_string();
                    name.push(".");
                    name.push(super::bundle::EXTENSION);
                    PathBuf::from(name)
                }
            })
        })
    }

    fn bundle_to_import(&self) -> Result<Option<PathBuf>, String> {
        run_dialog(
            r#"
            try
                set chosenFile to choose file with prompt "Choose a neati session bundle"
                return POSIX path of chosenFile
            on error number -128
                return ""
            end try
        "#,
        )
    }

    fn project_folder(&self) -> Result<Option<PathBuf>, String> {
        run_dialog(
            r#"
            try
                set chosenFolder to choose folder with prompt "Choose the project folder on this Mac"
                return POSIX path of chosenFolder
            on error number -128
                return ""
            end try
        "#,
        )
    }
}
