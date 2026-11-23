use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeveloperWorkspace {
    pub id: String,
    pub name: String,
    pub display_path: String,
    /// The canonical folder, so the interface can remember it in settings.
    /// A saved path grants nothing: it is validated again before every use.
    pub path: String,
}

/// A likely project folder in the home directory (#486). A hint only:
/// nothing is scanned until the person adds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeveloperWorkspaceSuggestion {
    pub path: String,
    pub display_path: String,
    /// Direct children that carry a project marker, capped by the probe.
    pub project_count: u32,
}

/// The saved project folders that were registered again at launch, and the
/// ones that no longer validate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeveloperWorkspaceRestore {
    pub workspaces: Vec<DeveloperWorkspace>,
    pub refused: Vec<DeveloperWorkspaceRefusal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeveloperWorkspaceRefusal {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DeveloperEcosystem {
    Rust,
    Node,
    Python,
    Go,
    Java,
    Kotlin,
    Php,
    Ruby,
    Dotnet,
    C,
    Cpp,
    Swift,
    Dart,
    Elixir,
    Erlang,
    Scala,
    Clojure,
    Haskell,
    Zig,
    ObjectiveC,
    R,
    Julia,
    Lua,
    Terraform,
    Unity,
    Unreal,
    Godot,
    Elm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DeveloperArtifactKind {
    CargoTarget,
    NodeModules,
    SvelteKitOutput,
    NextOutput,
    SvelteKitTypes,
    NextWebpackCache,
    PythonVenv,
    GoModuleCache,
    MavenTarget,
    SbtTarget,
    ClojureTarget,
    GradleBuild,
    GradleCache,
    ComposerVendor,
    RubyBundle,
    DotnetBin,
    DotnetObj,
    CMakeBuild,
    SwiftBuild,
    FlutterTooling,
    ElixirBuild,
    ElixirDeps,
    ErlangBuild,
    HaskellStackWork,
    HaskellDistNewstyle,
    ZigCache,
    TerraformCache,
    // Project-local caches kondo and npkill find (#479). Each is recognized
    // only with direct project evidence, stays reviewable, and is never a
    // one-button build output (`preselection::is_build_output`).
    PythonBytecodeCache,
    PytestCache,
    MypyCache,
    RuffCache,
    ToxEnvironments,
    NoxEnvironments,
    TurboCache,
    AngularCache,
    NxCache,
    UnityLibrary,
    UnityTemp,
    UnityObj,
    UnityLogs,
    UnrealIntermediate,
    UnrealDerivedDataCache,
    GodotCache,
    ElmStuff,
    ClojureCpcache,
    BloopCache,
    MetalsCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DeveloperArtifactStatus {
    /// Every measured entry and project marker was verified.
    Complete,
    /// The generated-folder scope and project evidence are verified, but one
    /// or more descendants could not be measured. Manual cleanup also requires
    /// independent complete ownership evidence and execution-time revalidation.
    MeasurementIncomplete,
    /// A safety boundary (for example a symlink, filesystem boundary, or
    /// project marker identity) could not be verified. Cleanup is forbidden.
    SafetyBlocked,
    /// The scan was cancelled before this artifact could be fully validated.
    ScanCancelled,
    /// Observed output whose owner-use and removal contracts are not verified.
    ObservationOnly,
}

impl DeveloperArtifactStatus {
    pub fn allows_manual_cleanup(self) -> bool {
        matches!(self, Self::Complete | Self::MeasurementIncomplete)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeveloperArtifact {
    pub id: String,
    pub workspace_id: String,
    pub project_name: String,
    pub ecosystem: DeveloperEcosystem,
    pub kind: DeveloperArtifactKind,
    pub path: String,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub logical_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub allocated_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub file_count: u64,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub newest_mtime: Option<u64>,
    pub rebuild_hint: Option<String>,
    pub evidence: Vec<String>,
    pub ownership: crate::models::ArtifactOwnershipEvidence,
    pub status: DeveloperArtifactStatus,
    pub incomplete_reason: Option<String>,
    pub selected_by_default: bool,
    /// When the project that owns this artifact was last worked on, from
    /// cheap metadata (#504). `None` when it was not observed.
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub project_last_activity: Option<u64>,
    /// Why one-button Clean includes this artifact or leaves it for review.
    pub clean_inclusion: CleanInclusion,
}

/// Why one-button Clean includes a project artifact or leaves it for review
/// (#494, #504). `Included` is the only state with `selected_by_default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CleanInclusion {
    /// Included: every rule and the exact use check passed.
    Included,
    /// This kind is always reviewed by a person.
    NotOffered,
    /// Measurement or generated-only ownership is not verified.
    NotVerified,
    /// A dependency store, and the dependency rule is off.
    RuleOff,
    /// A dependency store without its ecosystem's lockfile beside it.
    NoLockfile,
    /// The project was worked on more recently than the rule allows, or a
    /// running process works inside it.
    RecentlyActive,
    /// The project's last activity could not be observed.
    ActivityUnknown,
    /// A running process has files open in it.
    InUse,
    /// The open-file check could not prove it idle.
    UseUnknown,
}

/// Why a directory the scan wanted to inspect was left uninspected.
///
/// The scan cannot separate "the user answered Don't Allow" from "the read was
/// refused or cancelled": both reach the app as the same access refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DeveloperArtifactUninspectedReason {
    /// The operating system refused access to the folder, including a denied
    /// or cancelled folder-access prompt.
    PermissionDenied,
    /// The folder could not be read for any other I/O reason.
    Unreadable,
}

impl DeveloperArtifactUninspectedReason {
    pub fn user_message(self, name: &str) -> String {
        match self {
            Self::PermissionDenied => format!(
                "{} was not inspected because the operating system refused access. Allow access in System Settings, then scan again.",
                name
            ),
            Self::Unreadable => {
                format!("{} was not inspected because it could not be read.", name)
            }
        }
    }
}

/// A directory the whole-home scan could not inspect.
///
/// Recorded so a partial scan names what it missed and offers a retry, instead
/// of folding a refused folder into an anonymous skip counter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeveloperArtifactUninspected {
    pub path: String,
    pub name: String,
    pub reason: DeveloperArtifactUninspectedReason,
    /// Whether granting access and scanning again can include this directory.
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DeveloperArtifactScanResult {
    pub scan_id: String,
    pub items: Vec<DeveloperArtifact>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub discovered_count: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub measured_count: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped_entries: u64,
    pub cancelled: bool,
    pub truncated: bool,
    /// Directories the walk could not inspect, with the reason and whether a
    /// retry can include them.
    #[serde(default)]
    pub uninspected: Vec<DeveloperArtifactUninspected>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeveloperArtifactScanEvent {
    Started {
        scan_id: String,
        #[serde(with = "crate::ipc_numeric::u64")]
        #[specta(type = u64)]
        workspace_count: u64,
    },
    WorkspaceStarted {
        workspace: DeveloperWorkspace,
    },
    ProjectDiscovered {
        workspace_id: String,
        project_name: String,
        ecosystem: DeveloperEcosystem,
    },
    ArtifactMeasurementStarted {
        artifact_id: String,
        project_name: String,
        kind: DeveloperArtifactKind,
    },
    ArtifactFound {
        artifact: DeveloperArtifact,
    },
    Progress {
        workspace_id: String,
        #[serde(with = "crate::ipc_numeric::u64")]
        #[specta(type = u64)]
        discovered_count: u64,
        #[serde(with = "crate::ipc_numeric::u64")]
        #[specta(type = u64)]
        measured_count: u64,
        #[serde(with = "crate::ipc_numeric::u64")]
        #[specta(type = u64)]
        skipped_entries: u64,
    },
    WorkspaceFinished {
        workspace_id: String,
    },
    /// A directory the walk could not inspect, reported while the scan runs so
    /// a refused folder is visible without waiting for the final result.
    Uninspected {
        path: String,
        name: String,
        reason: DeveloperArtifactUninspectedReason,
        retryable: bool,
    },
    Finished {
        result: DeveloperArtifactScanResult,
    },
    Cancelled {
        scan_id: String,
    },
}
