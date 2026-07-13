//! Statement adjustment records.

/// One explicit statement adjustment kept outside source postings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementAdjustment {
    /// Operator-facing adjustment label.
    pub label: String,
    /// Signed exact minor-unit amount.
    pub amount_minor: i64,
}
