//! Counting likely secret shapes in session bytes (#509).
//!
//! A bundle carries transcripts, and transcripts carry whatever a tool printed.
//! Before a bundle is written, every byte passes through [`SecretScanner`],
//! which counts matches per [`SecretKind`] and keeps nothing else: no value,
//! no offset, no surrounding text. The shapes are conservative token prefixes
//! with a minimum length, preceded by a non-token character (or a JSON `\n`,
//! `\r`, `\t` escape) so `task-…` never reads as `sk-…`.
//!
//! Files are fed in chunks. A short tail of each chunk is carried into the
//! next, a match that touches the end of a chunk is counted only once the next
//! chunk shows where it ends, and a match lying wholly in the carried tail was
//! already counted. A token longer than the carry limit may be missed; counts
//! are a warning, not a proof of absence.

use std::sync::LazyLock;

use regex::bytes::Regex;

use crate::models::{SecretFinding, SecretKind};

static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:(?:\\[nrt]|[^A-Za-z0-9_\-])(?:",
        r"(sk-ant-[A-Za-z0-9_\-]{20,})",
        r"|(sk-[A-Za-z0-9_\-]{20,})",
        r"|(ghp_[A-Za-z0-9]{36})",
        r"|(github_pat_[A-Za-z0-9_]{22,})",
        r"|(AKIA[0-9A-Z]{16})",
        r"|(xox[bp]-[A-Za-z0-9\-]{10,})",
        r"))|(-----BEGIN [A-Z0-9 ]{0,40}PRIVATE KEY-----)",
    ))
    .expect("the secret pattern compiles")
});

/// Capture group → kind, in pattern order.
const GROUPS: [SecretKind; 7] = [
    SecretKind::AnthropicKey,
    SecretKind::OpenaiStyleKey,
    SecretKind::GithubToken,
    SecretKind::GithubFineGrainedToken,
    SecretKind::AwsAccessKey,
    SecretKind::SlackToken,
    SecretKind::PrivateKey,
];

/// Bytes always carried into the next chunk.
const OVERLAP: usize = 256;
/// The longest unfinished match carried forward.
const MAX_CARRY: usize = 16 * 1024;

/// Counts per kind, in [`SecretKind::ALL`] order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SecretCounts([u32; 7]);

impl SecretCounts {
    fn add(&mut self, kind: SecretKind) {
        let index = SecretKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
            .expect("every kind is listed");
        self.0[index] = self.0[index].saturating_add(1);
    }

    pub fn merge(&mut self, other: &SecretCounts) {
        for (mine, theirs) in self.0.iter_mut().zip(other.0) {
            *mine = mine.saturating_add(theirs);
        }
    }

    pub fn count(&self, kind: SecretKind) -> u32 {
        SecretKind::ALL
            .iter()
            .position(|candidate| *candidate == kind)
            .map_or(0, |index| self.0[index])
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|count| *count == 0)
    }

    /// The kinds found, with their counts.
    pub fn findings(&self) -> Vec<SecretFinding> {
        SecretKind::ALL
            .iter()
            .zip(self.0)
            .filter(|(_, count)| *count > 0)
            .map(|(kind, count)| SecretFinding { kind: *kind, count })
            .collect()
    }
}

/// A streaming counter over one file's bytes.
pub struct SecretScanner {
    carry: Vec<u8>,
    /// The carry ends with a match that was not counted yet.
    pending: bool,
    counts: SecretCounts,
}

impl Default for SecretScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretScanner {
    pub fn new() -> Self {
        // A virtual line break lets a token at the very start of a file match.
        Self {
            carry: vec![b'\n'],
            pending: false,
            counts: SecretCounts::default(),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let carried = self.carry.len();
        let pending = std::mem::replace(&mut self.pending, false);
        let mut buffer = std::mem::take(&mut self.carry);
        buffer.extend_from_slice(chunk);
        let mut pending_from = None;
        for captures in PATTERN.captures_iter(&buffer) {
            let whole = captures.get(0).expect("a match has a span");
            // Counted with the previous chunk, unless it was left pending.
            if whole.end() < carried || (whole.end() == carried && !pending) {
                continue;
            }
            if whole.end() == buffer.len() {
                // It may continue in the next chunk.
                pending_from = Some(whole.start());
                break;
            }
            self.count(&captures);
        }
        let mut keep_from = buffer.len().saturating_sub(OVERLAP);
        if let Some(start) = pending_from {
            let floor = buffer.len().saturating_sub(MAX_CARRY);
            if start >= floor {
                keep_from = keep_from.min(start);
                self.pending = true;
            }
        }
        self.carry = buffer.split_off(keep_from);
    }

    fn count(&mut self, captures: &regex::bytes::Captures<'_>) {
        if let Some(kind) = GROUPS
            .iter()
            .enumerate()
            .find_map(|(index, kind)| captures.get(index + 1).map(|_| *kind))
        {
            self.counts.add(kind);
        }
    }

    pub fn finish(mut self) -> SecretCounts {
        if self.pending {
            // The file ended where the pending match did.
            let carried = std::mem::take(&mut self.carry);
            if let Some(captures) = PATTERN.captures_iter(&carried).find(|captures| {
                captures
                    .get(0)
                    .is_some_and(|whole| whole.end() == carried.len())
            }) {
                self.count(&captures);
            }
        }
        self.counts
    }
}

/// Counts in one complete byte string.
pub fn count_in(bytes: &[u8]) -> SecretCounts {
    let mut scanner = SecretScanner::new();
    scanner.feed(bytes);
    scanner.finish()
}

/// A token-shaped test value, built at run time so neati's own sources never
/// read as holding a credential (the AI Control Center scans them).
#[cfg(test)]
pub(crate) fn fake_token(prefix: &str, length: usize) -> String {
    let body: String = "Z9".chars().cycle().take(length).collect();
    format!("{prefix}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anthropic() -> String {
        fake_token("sk-ant-", 32)
    }

    fn github() -> String {
        fake_token("ghp_", 36)
    }

    fn sample() -> String {
        format!(
            concat!(
                "{{\"text\":\"key {anthropic} and OPENAI={openai}\\n",
                "{github}\\n{pat} {aws} ",
                "{slack_bot} {slack_user} {private_key}\\n\"}}\n"
            ),
            anthropic = anthropic(),
            openai = fake_token("sk-proj-", 24),
            github = github(),
            pat = fake_token("github_pat_11", 24),
            aws = fake_token("AKIA", 16),
            slack_bot = fake_token("xoxb-", 14),
            slack_user = fake_token("xoxp-", 14),
            private_key = ["-----BEGIN ", "OPENSSH PRIVATE", " KEY-----"].concat(),
        )
    }

    #[test]
    fn each_kind_is_counted_and_no_value_is_kept() {
        let counts = count_in(sample().as_bytes());
        assert_eq!(counts.count(SecretKind::AnthropicKey), 1);
        assert_eq!(counts.count(SecretKind::OpenaiStyleKey), 1);
        assert_eq!(counts.count(SecretKind::GithubToken), 1);
        assert_eq!(counts.count(SecretKind::GithubFineGrainedToken), 1);
        assert_eq!(counts.count(SecretKind::AwsAccessKey), 1);
        assert_eq!(counts.count(SecretKind::SlackToken), 2);
        assert_eq!(counts.count(SecretKind::PrivateKey), 1);
        let findings = serde_json::to_string(&counts.findings()).expect("serializes");
        assert!(!findings.contains(&anthropic()));
        assert!(!findings.contains(&github()));
    }

    #[test]
    fn lookalikes_are_not_counted() {
        let text = [
            format!("ta{}", fake_token("sk-", 26)),
            format!("ri{}", fake_token("sk-", 22)),
            "sk-short".to_string(),
            "ghp_short".to_string(),
            fake_token("xoxa-", 10),
            format!("de{}", fake_token("sk-", 26)),
        ]
        .join(" ");
        assert!(
            count_in(text.as_bytes()).is_empty(),
            "{:?}",
            count_in(text.as_bytes())
        );
    }

    #[test]
    fn a_token_at_the_start_of_a_file_counts() {
        assert_eq!(
            count_in(anthropic().as_bytes()).count(SecretKind::AnthropicKey),
            1
        );
    }

    #[test]
    fn chunk_boundaries_neither_lose_nor_double_count() {
        let text = sample().repeat(7);
        let expected = count_in(text.as_bytes());
        for size in [1, 2, 3, 7, 13, 31, 64, 255, 256, 257, 1000] {
            let mut scanner = SecretScanner::new();
            for chunk in text.as_bytes().chunks(size) {
                scanner.feed(chunk);
            }
            assert_eq!(scanner.finish(), expected, "chunk size {size}");
        }
        assert_eq!(expected.count(SecretKind::SlackToken), 14);
    }
}
