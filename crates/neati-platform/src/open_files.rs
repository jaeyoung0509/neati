//! Bounded open-file observation for a known cache subtree. An unsuccessful or
//! incomplete lsof invocation never proves a cache idle.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenFileState {
    Idle,
    InUse,
    Unknown,
}

/// Injectable observation port; production reads lsof and fixtures state the
/// handle evidence without depending on the host operating system.
pub trait OpenFileProbe: Send + Sync + std::fmt::Debug {
    fn observe(&self, path: &Path) -> OpenFileState;
}

#[derive(Debug)]
pub struct NativeOpenFileProbe;
impl OpenFileProbe for NativeOpenFileProbe {
    fn observe(&self, path: &Path) -> OpenFileState {
        observe_open_files(path)
    }
}

#[derive(Debug)]
pub struct FixedOpenFileProbe(pub OpenFileState);
impl OpenFileProbe for FixedOpenFileProbe {
    fn observe(&self, _path: &Path) -> OpenFileState {
        self.0
    }
}

pub fn observe_open_files(path: &Path) -> OpenFileState {
    #[cfg(target_os = "macos")]
    {
        if !path.is_absolute() {
            return OpenFileState::Unknown;
        }
        let mut command = std::process::Command::new("/usr/sbin/lsof");
        command.args(["-nP", "-Fpn"]);
        if path.is_dir() {
            command.arg("+D");
        } else {
            command.arg("--");
        }
        command.arg(path);
        match crate::subprocess::run_with_timeout(command, std::time::Duration::from_secs(3)) {
            Ok(output) => classify(output.status.code(), &output.stdout, &output.stderr),
            Err(_) => OpenFileState::Unknown,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        OpenFileState::Unknown
    }
}

#[cfg(any(target_os = "macos", test))]
fn classify(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> OpenFileState {
    if !stderr.is_empty() {
        return OpenFileState::Unknown;
    }
    if code == Some(1) && stdout.is_empty() {
        return OpenFileState::Idle;
    }
    if matches!(code, Some(0 | 1))
        && stdout
            .split(|byte| *byte == b'\n')
            .any(|line| line.starts_with(b"n"))
    {
        return OpenFileState::InUse;
    }
    OpenFileState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_complete_empty_listing_proves_idle() {
        assert_eq!(classify(Some(1), b"", b""), OpenFileState::Idle);
        assert_eq!(
            classify(Some(0), b"p123\nn/cache/file\n", b""),
            OpenFileState::InUse
        );
        assert_eq!(
            classify(Some(1), b"", b"permission denied"),
            OpenFileState::Unknown
        );
        assert_eq!(classify(Some(0), b"", b""), OpenFileState::Unknown);
        assert_eq!(
            classify(Some(1), b"p123\nf3\nn/cache/file\n", b""),
            OpenFileState::InUse
        );
        assert_eq!(classify(None, b"", b""), OpenFileState::Unknown);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod native_tests {
    use super::*;
    #[test]
    fn native_probe_observes_an_open_fixture_and_its_closed_state() {
        let temp = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(temp.path().join("payload")).unwrap();
        assert_eq!(observe_open_files(temp.path()), OpenFileState::InUse);
        drop(file);
        assert_eq!(observe_open_files(temp.path()), OpenFileState::Idle);
    }
}
