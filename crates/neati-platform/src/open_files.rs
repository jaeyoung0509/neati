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
    observe_open_files_with_cancellation(path, &|| false)
}

/// Scan cancellation never proves idle. Planning and execution continue to use
/// the ordinary fresh observation above, without a reused scan verdict.
pub fn observe_open_files_with_cancellation(
    path: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> OpenFileState {
    if is_cancelled() {
        return OpenFileState::Unknown;
    }
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
        observe_command(command, is_cancelled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        OpenFileState::Unknown
    }
}

/// Directories observed by one lsof invocation. Measured on a development Mac,
/// 88 units in one call took 0.49 s against 7.3 s one at a time (#493).
const BATCH_UNITS: usize = 48;

/// Observes many units with as few lsof invocations as possible (#493).
///
/// Every unit still receives its own verdict, assigned by path-component
/// prefix of each open file lsof reports. A batch that writes anything to
/// stderr, exits abnormally or times out proves nothing: its units are then
/// observed one at a time, so a failed batch never becomes Idle. Cancellation
/// never proves idle either. Planning and execution do not use this; they keep
/// the fresh single-unit observation.
pub fn observe_open_files_batch(
    paths: &[std::path::PathBuf],
    is_cancelled: &dyn Fn() -> bool,
) -> Vec<OpenFileState> {
    let mut states = vec![OpenFileState::Unknown; paths.len()];
    // (index, canonical directory) of every unit lsof can read as a subtree.
    let mut batchable: Vec<(usize, std::path::PathBuf)> = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if is_cancelled() {
            return vec![OpenFileState::Unknown; paths.len()];
        }
        let canonical = path
            .is_absolute()
            .then(|| std::fs::canonicalize(path).ok())
            .flatten()
            .filter(|canonical| canonical.is_dir());
        match canonical {
            Some(canonical) => batchable.push((index, canonical)),
            None => states[index] = observe_open_files_with_cancellation(path, is_cancelled),
        }
    }
    for chunk in batchable.chunks(BATCH_UNITS) {
        if is_cancelled() {
            return vec![OpenFileState::Unknown; paths.len()];
        }
        let units: Vec<std::path::PathBuf> = chunk
            .iter()
            .map(|(_, canonical)| canonical.clone())
            .collect();
        match observe_batch_command(&units, is_cancelled) {
            Some(verdicts) => {
                for ((index, _), verdict) in chunk.iter().zip(verdicts) {
                    states[*index] = verdict;
                }
            }
            None => {
                for (index, _) in chunk {
                    states[*index] =
                        observe_open_files_with_cancellation(&paths[*index], is_cancelled);
                }
            }
        }
    }
    states
}

#[cfg(target_os = "macos")]
fn observe_batch_command(
    units: &[std::path::PathBuf],
    is_cancelled: &dyn Fn() -> bool,
) -> Option<Vec<OpenFileState>> {
    let mut command = std::process::Command::new("/usr/sbin/lsof");
    command.args(["-nP", "-Fpn"]);
    for unit in units {
        command.arg("+D").arg(unit);
    }
    // One extra second per sixteen units on top of the single-unit budget.
    let budget = std::time::Duration::from_secs(3 + (units.len() as u64).div_ceil(16));
    let output =
        crate::subprocess::run_with_timeout_cancellable(command, budget, is_cancelled).ok()?;
    assign_batch(units, output.status.code(), &output.stdout, &output.stderr)
}

#[cfg(not(target_os = "macos"))]
fn observe_batch_command(
    _units: &[std::path::PathBuf],
    _is_cancelled: &dyn Fn() -> bool,
) -> Option<Vec<OpenFileState>> {
    None
}

/// Splits one complete lsof field stream into per-unit verdicts, or returns
/// `None` when the stream cannot prove anything about the batch.
#[cfg(any(target_os = "macos", test))]
fn assign_batch(
    units: &[std::path::PathBuf],
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Option<Vec<OpenFileState>> {
    if !stderr.is_empty() || !matches!(code, Some(0 | 1)) {
        return None;
    }
    let open: Vec<&[u8]> = stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(b"n"))
        .collect();
    if code == Some(0) && open.is_empty() {
        // Success with no file names is not the complete empty listing that
        // proves idle.
        return None;
    }
    Some(
        units
            .iter()
            .map(|unit| {
                let unit = unit.as_os_str().as_encoded_bytes();
                let inside = |name: &&[u8]| {
                    name.starts_with(unit)
                        && (name.len() == unit.len() || name.get(unit.len()) == Some(&b'/'))
                };
                if open.iter().any(inside) {
                    OpenFileState::InUse
                } else {
                    OpenFileState::Idle
                }
            })
            .collect(),
    )
}

#[cfg(target_os = "macos")]
fn observe_command(
    command: std::process::Command,
    is_cancelled: &dyn Fn() -> bool,
) -> OpenFileState {
    match crate::subprocess::run_with_timeout_cancellable(
        command,
        std::time::Duration::from_secs(3),
        is_cancelled,
    ) {
        Ok(output) => classify(output.status.code(), &output.stdout, &output.stderr),
        Err(_) => OpenFileState::Unknown,
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
    fn a_cancelled_read_never_proves_the_unit_idle() {
        assert_eq!(
            observe_open_files_with_cancellation(Path::new("/fixture/not-probed"), &|| true),
            OpenFileState::Unknown
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn stop_reaps_an_owned_read_only_probe_and_returns_unknown() {
        let requested = std::sync::OnceLock::new();
        let started = std::time::Instant::now();
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & wait"]);
        let result = observe_command(command, &|| {
            if started.elapsed() < std::time::Duration::from_millis(100) {
                return false;
            }
            requested.get_or_init(std::time::Instant::now);
            true
        });
        assert_eq!(result, OpenFileState::Unknown);
        assert!(
            requested.get().expect("Stop was observed").elapsed()
                < std::time::Duration::from_secs(1)
        );
    }
    fn units(paths: &[&str]) -> Vec<std::path::PathBuf> {
        paths.iter().map(std::path::PathBuf::from).collect()
    }

    #[test]
    fn a_batch_assigns_each_open_file_to_its_own_unit() {
        let units = units(&["/c/app", "/c/app-helper", "/c/other"]);
        assert_eq!(
            assign_batch(&units, Some(1), b"", b""),
            Some(vec![OpenFileState::Idle; 3])
        );
        // A sibling whose name extends another unit's name is not inside it.
        let listing = b"p10\nn/c/app-helper/db\np11\nn/c/other\n";
        assert_eq!(
            assign_batch(&units, Some(0), listing, b""),
            Some(vec![
                OpenFileState::Idle,
                OpenFileState::InUse,
                OpenFileState::InUse
            ])
        );
    }

    #[test]
    fn a_batch_that_proves_nothing_is_rejected_for_single_observation() {
        let units = units(&["/c/app"]);
        assert_eq!(assign_batch(&units, Some(1), b"", b"lsof: WARNING"), None);
        assert_eq!(assign_batch(&units, Some(2), b"", b""), None);
        assert_eq!(assign_batch(&units, None, b"", b""), None);
        assert_eq!(assign_batch(&units, Some(0), b"p10\n", b""), None);
    }

    #[test]
    fn a_cancelled_batch_never_proves_a_unit_idle() {
        let paths = units(&["/fixture/a", "/fixture/b"]);
        assert_eq!(
            observe_open_files_batch(&paths, &|| true),
            vec![OpenFileState::Unknown; 2]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_native_batch_never_calls_a_held_unit_idle_or_an_untouched_one_in_use() {
        let temp = tempfile::tempdir().unwrap();
        let held = temp.path().join("held");
        let quiet = temp.path().join("quiet");
        std::fs::create_dir_all(&held).unwrap();
        std::fs::create_dir_all(&quiet).unwrap();
        std::fs::write(quiet.join("file"), b"x").unwrap();
        let _handle = std::fs::File::create(held.join("open")).unwrap();
        let states = observe_open_files_batch(&[held, quiet], &|| false);
        // A loaded runner may make lsof answer Unknown; it must never invert.
        assert_ne!(states[0], OpenFileState::Idle);
        assert_ne!(states[1], OpenFileState::InUse);
    }

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
