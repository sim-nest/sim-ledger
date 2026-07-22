//! Bookkeeping journal drafts and profile data for ledger sets.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod profile;

use std::fmt;

use serde::{Deserialize, Serialize};
use sim_ledger::Posting;
use time::Date;

pub use profile::{TaxProfile, VatRate, load_profile_str};

/// A reference-only attachment that supports a journal draft.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEvidenceRef {
    /// Source namespace, such as an office site, file codec, or ledger source.
    pub backend: String,
    /// Source-local identifier.
    pub external_id: String,
    /// Optional immutable source marker, such as an ETag or content digest.
    pub version: Option<String>,
    /// Optional operator-facing URL.
    pub web_url: Option<String>,
    /// Optional voucher or capture digest recorded with the draft.
    pub immutable_hint: Option<String>,
}

impl LedgerEvidenceRef {
    /// Builds a reference-only evidence attachment.
    #[must_use]
    pub fn new(
        backend: impl Into<String>,
        external_id: impl Into<String>,
        version: Option<String>,
        web_url: Option<String>,
        immutable_hint: Option<String>,
    ) -> Self {
        Self {
            backend: backend.into(),
            external_id: external_id.into(),
            version,
            web_url,
            immutable_hint,
        }
    }
}

/// Draft bookkeeping voucher before the operator commits it into a year file.
#[derive(Clone, Debug, PartialEq)]
pub struct JournalDraft {
    /// Voucher date.
    pub date: Date,
    /// Operator-facing voucher text.
    pub text: String,
    /// Exact posting lines.
    pub postings: Vec<Posting>,
    /// Reference-only evidence attachments.
    pub evidence: Vec<LedgerEvidenceRef>,
}

/// Journal category used for draft grouping and review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalCategory {
    /// Sales journal.
    Sales,
    /// Purchases journal.
    Purchases,
    /// Bank journal.
    Bank,
    /// Expense journal.
    Expense,
    /// Payroll summary journal.
    PayrollSummary,
    /// VAT journal.
    Vat,
    /// Manual journal.
    Manual,
    /// Opening journal.
    Opening,
    /// Closing journal.
    Closing,
}

/// Failure while checking bookkeeping draft data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BooksError {
    /// A draft must contain at least one posting line.
    EmptyDraft,
    /// Posting amounts do not sum to zero.
    Unbalanced {
        /// Signed minor-unit sum for the draft.
        minor_sum: i64,
    },
    /// Posting sums exceeded the exact minor-unit range.
    BalanceOverflow,
    /// Tax or VAT profile data could not be parsed.
    Profile(String),
}

impl fmt::Display for BooksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDraft => f.write_str("journal draft has no postings"),
            Self::Unbalanced { minor_sum } => {
                write!(f, "journal draft is unbalanced by {minor_sum} minor units")
            }
            Self::BalanceOverflow => f.write_str("journal draft balance overflows minor units"),
            Self::Profile(message) => write!(f, "profile error: {message}"),
        }
    }
}

impl std::error::Error for BooksError {}

/// Validate that a journal draft is non-empty and exactly balanced.
pub fn validate_draft(draft: &JournalDraft) -> Result<(), BooksError> {
    if draft.postings.is_empty() {
        return Err(BooksError::EmptyDraft);
    }
    let sum = minor_sum(&draft.postings)?;
    if sum != 0 {
        return Err(BooksError::Unbalanced { minor_sum: sum });
    }
    Ok(())
}

fn minor_sum(postings: &[Posting]) -> Result<i64, BooksError> {
    let sum = postings
        .iter()
        .map(|posting| i128::from(posting.amount.0))
        .sum::<i128>();
    i64::try_from(sum).map_err(|_| BooksError::BalanceOverflow)
}

#[cfg(test)]
mod tests {
    use sim_ledger::Amount;
    use time::Month;

    use super::*;

    // conformance: ledger libraries validate journal drafts and evidence links.

    #[test]
    fn balanced_draft_validates() {
        let draft = draft(vec![posting(100), posting(-100)]);

        validate_draft(&draft).unwrap();
    }

    #[test]
    fn unbalanced_draft_reports_minor_sum() {
        let draft = draft(vec![posting(100), posting(-70)]);

        let err = validate_draft(&draft).unwrap_err();
        assert_eq!(err, BooksError::Unbalanced { minor_sum: 30 });
    }

    #[test]
    fn evidence_refs_are_reference_only() {
        let evidence = LedgerEvidenceRef::new(
            "site/msgraph",
            "messages/msg-1",
            Some("etag-1".to_owned()),
            None,
            Some("digest-1".to_owned()),
        );

        assert_eq!(evidence.backend, "site/msgraph");
        assert_eq!(evidence.external_id, "messages/msg-1");
        assert_eq!(evidence.version.as_deref(), Some("etag-1"));
    }

    #[test]
    fn test_profile_loads_from_data_file() {
        let profile = load_profile_str(include_str!("../profiles/test-local.toml")).unwrap();

        assert_eq!(profile.id, "test-local");
        assert!(profile.vat_rates.iter().any(|rate| rate.code == "standard"));
    }

    fn draft(postings: Vec<Posting>) -> JournalDraft {
        JournalDraft {
            date: Date::from_calendar_date(2026, Month::July, 13).unwrap(),
            text: "Office bridge draft".to_owned(),
            postings,
            evidence: vec![LedgerEvidenceRef::new(
                "ledger-test",
                "fixture-1",
                None,
                None,
                None,
            )],
        }
    }

    fn posting(amount: i64) -> Posting {
        Posting {
            id: 0,
            source_id: None,
            voucher_id: 0,
            account: 1910,
            amount: Amount(amount),
            text: None,
        }
    }
}
