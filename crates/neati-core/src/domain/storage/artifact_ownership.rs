//! A generated-folder marker is not ownership evidence for its descendants.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOwnershipUncertainty {
    UnreadableMetadata,
    MalformedMetadata,
    MissingGit,
    TimedOut,
    Cancelled,
    BudgetExceeded,
    ChangedDuringProbe,
    OutsideScope,
    UnsupportedIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum ArtifactOwnershipEvidence {
    VerifiedGenerated,
    TrackedContent,
    NestedRepository,
    DeploymentKeyMaterial,
    Incomplete(ArtifactOwnershipUncertainty),
}

impl ArtifactOwnershipEvidence {
    pub fn allows_cleanup(self) -> bool {
        self == Self::VerifiedGenerated
    }

    pub fn refusal_message(self) -> Option<&'static str> {
        match self {
            Self::VerifiedGenerated => None,
            Self::TrackedContent => Some("Git-tracked content is inside this artifact; cleanup is unavailable."),
            Self::NestedRepository => Some("Nested repository or worktree metadata is inside this artifact; cleanup is unavailable."),
            Self::DeploymentKeyMaterial => Some("A deployment keypair filename is inside this artifact; cleanup is unavailable. Key contents were not read."),
            Self::Incomplete(reason) => Some(match reason {
                ArtifactOwnershipUncertainty::MissingGit => "Git ownership could not be checked because Git is unavailable; cleanup is unavailable.",
                ArtifactOwnershipUncertainty::TimedOut => "The ownership check timed out; cleanup is unavailable. Scan again.",
                ArtifactOwnershipUncertainty::Cancelled => "The ownership check was cancelled; cleanup is unavailable. Scan again.",
                ArtifactOwnershipUncertainty::BudgetExceeded => "The ownership check reached its inspection limit; cleanup is unavailable.",
                ArtifactOwnershipUncertainty::ChangedDuringProbe => "Project or artifact metadata changed during the ownership check; cleanup is unavailable. Scan again.",
                ArtifactOwnershipUncertainty::OutsideScope => "Artifact ownership could not be bound to the reviewed project; cleanup is unavailable.",
                ArtifactOwnershipUncertainty::UnsupportedIndex => "This repository index layout is not supported by the read-only ownership check; cleanup is unavailable.",
                ArtifactOwnershipUncertainty::UnreadableMetadata | ArtifactOwnershipUncertainty::MalformedMetadata => "Repository or artifact ownership metadata could not be verified; cleanup is unavailable.",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_complete_generated_evidence_authorizes_cleanup() {
        assert!(ArtifactOwnershipEvidence::VerifiedGenerated.allows_cleanup());
        for evidence in [
            ArtifactOwnershipEvidence::TrackedContent,
            ArtifactOwnershipEvidence::NestedRepository,
            ArtifactOwnershipEvidence::DeploymentKeyMaterial,
            ArtifactOwnershipEvidence::Incomplete(ArtifactOwnershipUncertainty::Cancelled),
        ] {
            assert!(!evidence.allows_cleanup());
            assert!(evidence.refusal_message().is_some());
        }
    }
}
