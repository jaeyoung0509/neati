//! Deliberate temporary-folder review is separate from automatic cache cleanup.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TemporaryUsageState {
    InUse,
    NoUseDetected,
    UnableToDetermine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TemporaryContentKind {
    BuildOutput,
    SourceCheckout,
    GitMetadata,
    BrowserOrSessionData,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TemporaryRemovalMode {
    WholeFolder,
    GeneratedSubtree,
}

/// The only uncertainty the temporary workflow may accept is usage. These
/// acknowledgements never override scope, ownership, identity or no-link checks.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type)]
pub struct TemporaryReviewConsent {
    pub confirmed: bool,
    pub accept_unknown_usage: bool,
    pub accept_source_loss: bool,
}

pub fn require_temporary_consent(
    consent: TemporaryReviewConsent,
    unknown_usage: bool,
    whole_folder: bool,
) -> Result<(), &'static str> {
    if !consent.confirmed {
        return Err("Confirm the exact reviewed temporary-folder selection first.");
    }
    if unknown_usage && !consent.accept_unknown_usage {
        return Err("Accept the listed usage uncertainty before moving these temporary folders.");
    }
    if whole_folder && !consent.accept_source_loss {
        return Err(
            "Acknowledge that whole folders may contain unpublished source, Git and session data.",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_consent_never_implies_confirmation_or_source_consent() {
        let mut consent = TemporaryReviewConsent::default();
        assert!(require_temporary_consent(consent, true, true).is_err());
        consent.confirmed = true;
        assert!(require_temporary_consent(consent, true, true).is_err());
        consent.accept_unknown_usage = true;
        assert!(require_temporary_consent(consent, true, true).is_err());
        consent.accept_source_loss = true;
        assert!(require_temporary_consent(consent, true, true).is_ok());
    }
}
