//! Exact, human-decided reconciliation and independently recomputable certificates.
//!
//! Candidate scores order review only. They are never accepted implicitly and
//! this module has no operation that posts a voucher or approves a correction.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Amount, CanonicalStatementRow, Posting, StatementSnapshot, is_balanced};

/// One immutable ledger movement presented for reconciliation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerMovement {
    /// Stable voucher identity in the frozen ledger snapshot.
    pub voucher_id: i64,
    /// Exact signed movement of the reconciled account.
    pub amount: Amount,
    /// Currency of the reconciled account.
    pub currency: String,
    /// ISO-8601 posting date.
    pub date: String,
    /// Optional bounded matching text.
    pub text: Option<String>,
}

/// Frozen statement and ledger inputs. The verifier receives this independently.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationInputs {
    /// Human-declared identity of this input set.
    pub snapshot_ref: String,
    /// Stable identity of the human who alone may decide this reconciliation.
    pub reviewer_ref: String,
    /// Canonical statement snapshot.
    pub statement: StatementSnapshot,
    /// Exact in-scope ledger movements.
    pub movements: Vec<LedgerMovement>,
}

/// Explicit limits for deterministic candidate enumeration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateBounds {
    /// Maximum total combinations examined.
    pub max_work: usize,
    /// Maximum candidates returned.
    pub max_results: usize,
    /// Maximum members on the grouped side (at least one).
    pub max_cardinality: usize,
    /// Maximum absolute date separation used by a candidate.
    pub max_date_distance_days: u32,
}

/// Supported exact candidate shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CandidateKind {
    /// One statement row to one ledger movement.
    OneToOne,
    /// One statement row to multiple ledger movements.
    OneToMany,
    /// Multiple statement rows to one ledger movement.
    ManyToOne,
}

/// A possible exact match. It is review material, never a decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Candidate shape.
    pub kind: CandidateKind,
    /// Exact statement row identities.
    pub row_ids: Vec<String>,
    /// Exact voucher identities.
    pub voucher_ids: Vec<i64>,
    /// Lower values appear first in review. This has no accounting authority.
    pub review_rank: u64,
}

/// Immutable references selected by Mia.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationRef {
    /// Exact statement row identities.
    pub row_ids: Vec<String>,
    /// Exact ledger voucher identities.
    pub voucher_ids: Vec<i64>,
}

/// Human-authored outcome for a reviewed set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionDisposition {
    /// The exact sets reconcile.
    Accept,
    /// The proposed relation was rejected.
    Reject,
    /// Mia split a proposed group; the refs name the accepted resulting part.
    Split,
    /// Mia merged proposed groups; the refs name the accepted resulting group.
    Merge,
    /// Review remains unresolved.
    Defer,
}

/// Immutable human decision record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// Stable journal/event identity.
    pub decision_ref: String,
    /// Human identity that authored the immutable decision.
    pub decided_by: String,
    /// Mia's disposition.
    pub disposition: DecisionDisposition,
    /// Exact sets reviewed or accepted.
    pub evidence: ReconciliationRef,
}

/// Accepted decision copied into a certificate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedDecision {
    /// Stable journal/event identity.
    pub decision_ref: String,
    /// Accept, split, or merge.
    pub disposition: DecisionDisposition,
    /// Exact sets covered by the decision.
    pub evidence: ReconciliationRef,
    /// Exact statement-side sum.
    pub row_total: Amount,
    /// Exact ledger-side sum.
    pub movement_total: Amount,
    /// Human authority copied from the validated input (prevents raw decisions
    /// from being used directly as posting authority through the Rust API).
    reviewer_ref: String,
}

/// Recomputable proof artifact. Derived fields are not trusted by the verifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationCertificate {
    /// Input identity asserted by the issuer.
    pub snapshot_ref: String,
    /// Exact cutoff asserted by the issuer.
    pub cutoff: String,
    /// Exact frozen source inputs bound into this certificate.
    pub source_inputs: ReconciliationInputs,
    /// Every immutable decision, including rejects and deferrals.
    pub decision_records: Vec<DecisionRecord>,
    /// Accepted human evidence.
    pub accepted: Vec<AcceptedDecision>,
    /// Statement rows not covered by accepted evidence.
    pub uncovered_rows: Vec<String>,
    /// Ledger movements not covered by accepted evidence.
    pub uncovered_vouchers: Vec<i64>,
    /// Statement snapshot total.
    pub statement_total: Amount,
    /// Sum of frozen ledger movements.
    pub ledger_total: Amount,
    /// Exact bridge residual (`statement_total - ledger_total`).
    pub residual: Amount,
    /// True only for complete coverage and zero residual.
    pub closed: bool,
}

/// Independently recomputed verification report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationReport {
    /// Recomputed certificate, byte-structure comparable to the supplied one.
    pub recomputed: ReconciliationCertificate,
}

/// A balanced correction proposal. Approval deliberately belongs outside this API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorrectionDraft {
    /// Human decision authorizing preparation (not posting).
    pub decision_ref: String,
    /// Exact balanced ledger posting lines.
    pub postings: Vec<Posting>,
}

/// Exact reconciliation refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconciliationError {
    /// A configured bound is zero or otherwise invalid.
    InvalidBounds,
    /// Candidate enumeration reached its explicit work bound.
    WorkBoundExceeded,
    /// An identity is empty or duplicated in an input snapshot.
    InvalidInput(&'static str),
    /// A decision reference is empty or occurs more than once.
    InvalidDecisionRef,
    /// A decision names a row or voucher outside the snapshot.
    UnknownEvidence,
    /// A decision was not authored by the declared human reviewer.
    WrongReviewer,
    /// Accepted evidence uses an item more than once.
    DuplicateCoverage,
    /// Accepted evidence has no row or no voucher.
    EmptyAcceptedSet,
    /// Exact arithmetic overflowed.
    ArithmeticOverflow,
    /// A certificate differs from independent recomputation.
    CertificateMismatch,
    /// Closing was requested with uncovered evidence or a nonzero residual.
    IncompleteClose,
    /// Correction lines are empty or not exactly balanced.
    UnbalancedCorrection,
    /// Only an accepted human decision may support a correction draft.
    CorrectionNotAccepted,
}

impl fmt::Display for ReconciliationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ReconciliationError {}

/// Deterministically enumerate exact one-to-one and bounded grouped candidates.
pub fn generate_candidates(
    inputs: &ReconciliationInputs,
    bounds: CandidateBounds,
) -> Result<Vec<Candidate>, ReconciliationError> {
    validate_inputs(inputs)?;
    if bounds.max_work == 0 || bounds.max_results == 0 || bounds.max_cardinality == 0 {
        return Err(ReconciliationError::InvalidBounds);
    }
    let rows = &inputs.statement.rows;
    let movements = &inputs.movements;
    let mut candidates = Vec::new();
    let mut work = 0usize;
    for row_size in 1..=bounds.max_cardinality.min(rows.len()) {
        for row_indices in combinations(rows.len(), row_size) {
            for movement_size in 1..=bounds.max_cardinality.min(movements.len()) {
                if row_size > 1 && movement_size > 1 {
                    continue;
                }
                for movement_indices in combinations(movements.len(), movement_size) {
                    work = work
                        .checked_add(1)
                        .ok_or(ReconciliationError::ArithmeticOverflow)?;
                    if work > bounds.max_work {
                        return Err(ReconciliationError::WorkBoundExceeded);
                    }
                    if exact_candidate(rows, movements, &row_indices, &movement_indices, bounds)? {
                        let kind = match (row_size, movement_size) {
                            (1, 1) => CandidateKind::OneToOne,
                            (1, _) => CandidateKind::OneToMany,
                            (_, 1) => CandidateKind::ManyToOne,
                            _ => unreachable!(),
                        };
                        let date_distance =
                            max_date_distance(rows, movements, &row_indices, &movement_indices)?;
                        let text_penalty =
                            text_penalty(rows, movements, &row_indices, &movement_indices);
                        candidates.push(Candidate {
                            kind,
                            row_ids: row_indices
                                .iter()
                                .map(|&i| rows[i].source_identity.clone())
                                .collect(),
                            voucher_ids: movement_indices
                                .iter()
                                .map(|&i| movements[i].voucher_id)
                                .collect(),
                            review_rank: u64::from(date_distance) * 2 + u64::from(text_penalty),
                        });
                    }
                }
            }
        }
    }
    candidates.sort_by(|a, b| {
        (a.review_rank, a.kind, &a.row_ids, &a.voucher_ids).cmp(&(
            b.review_rank,
            b.kind,
            &b.row_ids,
            &b.voucher_ids,
        ))
    });
    candidates.truncate(bounds.max_results);
    Ok(candidates)
}

/// Build a certificate solely from frozen inputs and immutable decisions.
pub fn build_certificate(
    inputs: &ReconciliationInputs,
    decisions: &[DecisionRecord],
    request_close: bool,
) -> Result<ReconciliationCertificate, ReconciliationError> {
    validate_inputs(inputs)?;
    let rows = inputs
        .statement
        .rows
        .iter()
        .map(|r| (r.source_identity.as_str(), r))
        .collect::<BTreeMap<_, _>>();
    let movements = inputs
        .movements
        .iter()
        .map(|m| (m.voucher_id, m))
        .collect::<BTreeMap<_, _>>();
    let mut decision_refs = BTreeSet::new();
    let mut covered_rows = BTreeSet::new();
    let mut covered_vouchers = BTreeSet::new();
    let mut accepted = Vec::new();
    for decision in decisions {
        if decision.decision_ref.is_empty() || !decision_refs.insert(&decision.decision_ref) {
            return Err(ReconciliationError::InvalidDecisionRef);
        }
        if decision.decided_by != inputs.reviewer_ref {
            return Err(ReconciliationError::WrongReviewer);
        }
        let is_accepted = matches!(
            decision.disposition,
            DecisionDisposition::Accept | DecisionDisposition::Split | DecisionDisposition::Merge
        );
        if !is_accepted {
            continue;
        }
        if decision.evidence.row_ids.is_empty() || decision.evidence.voucher_ids.is_empty() {
            return Err(ReconciliationError::EmptyAcceptedSet);
        }
        let mut row_total = 0_i64;
        for id in &decision.evidence.row_ids {
            let row = rows
                .get(id.as_str())
                .ok_or(ReconciliationError::UnknownEvidence)?;
            if !covered_rows.insert(id.clone()) {
                return Err(ReconciliationError::DuplicateCoverage);
            }
            row_total = row_total
                .checked_add(row.amount.0)
                .ok_or(ReconciliationError::ArithmeticOverflow)?;
        }
        let mut movement_total = 0_i64;
        for id in &decision.evidence.voucher_ids {
            let movement = movements
                .get(id)
                .ok_or(ReconciliationError::UnknownEvidence)?;
            if !covered_vouchers.insert(*id) {
                return Err(ReconciliationError::DuplicateCoverage);
            }
            movement_total = movement_total
                .checked_add(movement.amount.0)
                .ok_or(ReconciliationError::ArithmeticOverflow)?;
        }
        if row_total != movement_total {
            return Err(ReconciliationError::UnknownEvidence);
        }
        accepted.push(AcceptedDecision {
            decision_ref: decision.decision_ref.clone(),
            disposition: decision.disposition,
            evidence: decision.evidence.clone(),
            row_total: Amount(row_total),
            movement_total: Amount(movement_total),
            reviewer_ref: inputs.reviewer_ref.clone(),
        });
    }
    let uncovered_rows = rows
        .keys()
        .filter(|id| !covered_rows.contains(**id))
        .map(|id| (*id).to_owned())
        .collect::<Vec<_>>();
    let uncovered_vouchers = movements
        .keys()
        .filter(|id| !covered_vouchers.contains(id))
        .copied()
        .collect::<Vec<_>>();
    let ledger_total = checked_sum(inputs.movements.iter().map(|m| m.amount.0))?;
    let residual = inputs
        .statement
        .total
        .0
        .checked_sub(ledger_total)
        .ok_or(ReconciliationError::ArithmeticOverflow)?;
    let closed = uncovered_rows.is_empty() && uncovered_vouchers.is_empty() && residual == 0;
    if request_close && !closed {
        return Err(ReconciliationError::IncompleteClose);
    }
    Ok(ReconciliationCertificate {
        snapshot_ref: inputs.snapshot_ref.clone(),
        cutoff: inputs.statement.cutoff.clone(),
        source_inputs: inputs.clone(),
        decision_records: decisions.to_vec(),
        accepted,
        uncovered_rows,
        uncovered_vouchers,
        statement_total: inputs.statement.total,
        ledger_total: Amount(ledger_total),
        residual: Amount(residual),
        closed,
    })
}

/// Reload inputs and decisions, recompute every derived field, and compare exactly.
pub fn verify_certificate(
    inputs: &ReconciliationInputs,
    decisions: &[DecisionRecord],
    certificate: &ReconciliationCertificate,
) -> Result<ReconciliationReport, ReconciliationError> {
    if &certificate.source_inputs != inputs || certificate.decision_records != decisions {
        return Err(ReconciliationError::CertificateMismatch);
    }
    let recomputed = build_certificate(inputs, decisions, certificate.closed)?;
    if &recomputed != certificate {
        return Err(ReconciliationError::CertificateMismatch);
    }
    Ok(ReconciliationReport { recomputed })
}

/// Prepare an exactly balanced correction; this function cannot approve or post it.
pub fn prepare_correction_draft(
    decision: &AcceptedDecision,
    postings: Vec<Posting>,
) -> Result<CorrectionDraft, ReconciliationError> {
    if postings.is_empty() || !is_balanced(&postings) {
        return Err(ReconciliationError::UnbalancedCorrection);
    }
    Ok(CorrectionDraft {
        decision_ref: decision.decision_ref.clone(),
        postings,
    })
}

fn validate_inputs(inputs: &ReconciliationInputs) -> Result<(), ReconciliationError> {
    if inputs.snapshot_ref.is_empty()
        || inputs.reviewer_ref.is_empty()
        || inputs.statement.cutoff.is_empty()
    {
        return Err(ReconciliationError::InvalidInput(
            "empty snapshot identity or cutoff",
        ));
    }
    let mut row_ids = BTreeSet::new();
    let mut currency: Option<&str> = None;
    for row in &inputs.statement.rows {
        if row.source_identity.is_empty() || !row_ids.insert(&row.source_identity) {
            return Err(ReconciliationError::InvalidInput(
                "duplicate statement row identity",
            ));
        }
        if row.currency.is_empty() {
            return Err(ReconciliationError::InvalidInput("empty currency"));
        }
        match currency {
            Some(expected) if expected != row.currency => {
                return Err(ReconciliationError::InvalidInput("mixed currency"));
            }
            None => currency = Some(&row.currency),
            _ => {}
        }
    }
    let mut voucher_ids = BTreeSet::new();
    for movement in &inputs.movements {
        if movement.currency.is_empty() || !voucher_ids.insert(movement.voucher_id) {
            return Err(ReconciliationError::InvalidInput(
                "duplicate ledger voucher identity",
            ));
        }
        match currency {
            Some(expected) if expected != movement.currency => {
                return Err(ReconciliationError::InvalidInput("mixed currency"));
            }
            None => currency = Some(&movement.currency),
            _ => {}
        }
    }
    let total = checked_sum(inputs.statement.rows.iter().map(|r| r.amount.0))?;
    if total != inputs.statement.total.0 {
        return Err(ReconciliationError::InvalidInput(
            "fabricated statement total",
        ));
    }
    Ok(())
}

fn exact_candidate(
    rows: &[CanonicalStatementRow],
    movements: &[LedgerMovement],
    ri: &[usize],
    mi: &[usize],
    bounds: CandidateBounds,
) -> Result<bool, ReconciliationError> {
    let currency = &rows[ri[0]].currency;
    if ri.iter().any(|&i| &rows[i].currency != currency)
        || mi.iter().any(|&i| &movements[i].currency != currency)
    {
        return Ok(false);
    }
    let row_total = checked_sum(ri.iter().map(|&i| rows[i].amount.0))?;
    let movement_total = checked_sum(mi.iter().map(|&i| movements[i].amount.0))?;
    Ok(row_total == movement_total
        && max_date_distance(rows, movements, ri, mi)? <= bounds.max_date_distance_days)
}

fn max_date_distance(
    rows: &[CanonicalStatementRow],
    movements: &[LedgerMovement],
    ri: &[usize],
    mi: &[usize],
) -> Result<u32, ReconciliationError> {
    let mut max = 0;
    for &r in ri {
        for &m in mi {
            max = max.max(date_distance(&rows[r].date, &movements[m].date)?);
        }
    }
    Ok(max)
}

fn text_penalty(
    rows: &[CanonicalStatementRow],
    movements: &[LedgerMovement],
    ri: &[usize],
    mi: &[usize],
) -> u32 {
    let row_words = ri
        .iter()
        .filter_map(|&i| rows[i].description.as_deref())
        .flat_map(words)
        .collect::<BTreeSet<_>>();
    let movement_words = mi
        .iter()
        .filter_map(|&i| movements[i].text.as_deref())
        .flat_map(words)
        .collect::<BTreeSet<_>>();
    u32::from(row_words.is_disjoint(&movement_words))
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
}

fn checked_sum(mut values: impl Iterator<Item = i64>) -> Result<i64, ReconciliationError> {
    values.try_fold(0_i64, |sum, value| {
        sum.checked_add(value)
            .ok_or(ReconciliationError::ArithmeticOverflow)
    })
}

fn combinations(len: usize, size: usize) -> Vec<Vec<usize>> {
    fn visit(
        start: usize,
        len: usize,
        left: usize,
        current: &mut Vec<usize>,
        out: &mut Vec<Vec<usize>>,
    ) {
        if left == 0 {
            out.push(current.clone());
            return;
        }
        for index in start..=len - left {
            current.push(index);
            visit(index + 1, len, left - 1, current, out);
            current.pop();
        }
    }
    if size == 0 || size > len {
        return Vec::new();
    }
    let mut out = Vec::new();
    visit(0, len, size, &mut Vec::new(), &mut out);
    out
}

fn date_distance(left: &str, right: &str) -> Result<u32, ReconciliationError> {
    let ordinal = |date: &str| -> Result<i64, ReconciliationError> {
        let mut parts = date.split('-').map(str::parse::<i64>);
        let year = parts
            .next()
            .and_then(Result::ok)
            .ok_or(ReconciliationError::InvalidInput("invalid date"))?;
        let month = parts
            .next()
            .and_then(Result::ok)
            .ok_or(ReconciliationError::InvalidInput("invalid date"))?;
        let day = parts
            .next()
            .and_then(Result::ok)
            .ok_or(ReconciliationError::InvalidInput("invalid date"))?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(ReconciliationError::InvalidInput("invalid date"));
        }
        let adjusted = year - i64::from(month <= 2);
        let era = adjusted.div_euclid(400);
        let yoe = adjusted - era * 400;
        let shifted_month = month + if month > 2 { -3 } else { 9 };
        Ok(
            era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * shifted_month + 2) / 5 + day
                - 1,
        )
    };
    u32::try_from((ordinal(left)? - ordinal(right)?).abs())
        .map_err(|_| ReconciliationError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CANONICAL_STATEMENT_ROW_VERSION, LedgerBalanceAtCutoff};

    fn row(id: &str, date: &str, amount: i64, text: &str) -> CanonicalStatementRow {
        CanonicalStatementRow {
            version: CANONICAL_STATEMENT_ROW_VERSION,
            date: date.into(),
            amount: Amount(amount),
            currency: "SEK".into(),
            description: Some(text.into()),
            source_identity: id.into(),
            source_ref: "bank-export".into(),
            ordinal: 1,
            profile_id: "bank-v1".into(),
            original_bytes: id.as_bytes().to_vec(),
        }
    }
    fn movement(id: i64, date: &str, amount: i64, text: &str) -> LedgerMovement {
        LedgerMovement {
            voucher_id: id,
            amount: Amount(amount),
            currency: "SEK".into(),
            date: date.into(),
            text: Some(text.into()),
        }
    }
    fn inputs() -> ReconciliationInputs {
        let rows = vec![
            row("fee", "2026-01-02", -10, "bank fee"),
            row("transfer", "2026-01-03", -100, "transfer"),
            row("dep-a", "2026-01-04", 40, "deposit"),
            row("dep-b", "2026-01-04", 60, "deposit"),
        ];
        ReconciliationInputs {
            snapshot_ref: "sha256:fixture".into(),
            reviewer_ref: "mia".into(),
            statement: StatementSnapshot {
                cutoff: "2026-01-31".into(),
                source_ref: "bank-export".into(),
                profile_id: "bank-v1".into(),
                total: Amount(-10),
                rows,
                ledger_balances: vec![LedgerBalanceAtCutoff {
                    account: "1930".into(),
                    amount: Amount(-10),
                }],
            },
            movements: vec![
                movement(1, "2026-01-02", -10, "fee reversal"),
                movement(2, "2026-01-03", -40, "transfer"),
                movement(3, "2026-01-03", -60, "transfer"),
                movement(4, "2026-01-04", 100, "grouped deposit"),
            ],
        }
    }
    fn decisions() -> Vec<DecisionRecord> {
        vec![
            decision("d-fee", DecisionDisposition::Accept, &["fee"], &[1]),
            decision(
                "d-transfer",
                DecisionDisposition::Merge,
                &["transfer"],
                &[2, 3],
            ),
            decision(
                "d-deposit",
                DecisionDisposition::Split,
                &["dep-a", "dep-b"],
                &[4],
            ),
        ]
    }
    fn decision(
        id: &str,
        disposition: DecisionDisposition,
        rows: &[&str],
        vouchers: &[i64],
    ) -> DecisionRecord {
        DecisionRecord {
            decision_ref: id.into(),
            decided_by: "mia".into(),
            disposition,
            evidence: ReconciliationRef {
                row_ids: rows.iter().map(|s| (*s).into()).collect(),
                voucher_ids: vouchers.to_vec(),
            },
        }
    }

    #[test]
    fn candidates_are_exact_bounded_and_ranking_has_no_authority() {
        let candidates = generate_candidates(
            &inputs(),
            CandidateBounds {
                max_work: 200,
                max_results: 20,
                max_cardinality: 2,
                max_date_distance_days: 2,
            },
        )
        .unwrap();
        assert!(
            candidates
                .iter()
                .any(|c| c.kind == CandidateKind::OneToOne && c.row_ids == ["fee"])
        );
        assert!(
            candidates
                .iter()
                .any(|c| c.kind == CandidateKind::OneToMany && c.row_ids == ["transfer"])
        );
        assert!(
            candidates
                .iter()
                .any(|c| c.kind == CandidateKind::ManyToOne && c.row_ids == ["dep-a", "dep-b"])
        );
        assert!(build_certificate(&inputs(), &[], true).is_err());
        assert_eq!(
            generate_candidates(
                &inputs(),
                CandidateBounds {
                    max_work: 1,
                    max_results: 20,
                    max_cardinality: 2,
                    max_date_distance_days: 2
                }
            ),
            Err(ReconciliationError::WorkBoundExceeded)
        );
    }

    #[test]
    fn real_closed_fixture_verifies_from_empty_derived_state() {
        let certificate = build_certificate(&inputs(), &decisions(), true).unwrap();
        assert!(certificate.closed);
        assert_eq!(certificate.residual, Amount(0));
        let report = verify_certificate(&inputs(), &decisions(), &certificate).unwrap();
        assert_eq!(report.recomputed, certificate);
    }

    #[test]
    fn every_source_or_derived_mutation_breaks_independent_verification() {
        let source = inputs();
        let decisions = decisions();
        let certificate = build_certificate(&source, &decisions, true).unwrap();
        let mut mutations: Vec<(
            ReconciliationInputs,
            Vec<DecisionRecord>,
            ReconciliationCertificate,
        )> = Vec::new();
        let mut altered = source.clone();
        altered.statement.rows[0].amount = Amount(-11);
        altered.statement.total = Amount(-11);
        mutations.push((altered, decisions.clone(), certificate.clone()));
        let mut cutoff = source.clone();
        cutoff.statement.cutoff = "2026-02-01".into();
        mutations.push((cutoff, decisions.clone(), certificate.clone()));
        let mut voucher = source.clone();
        voucher.movements[0].amount = Amount(-11);
        mutations.push((voucher, decisions.clone(), certificate.clone()));
        let mut changed_decision = decisions.clone();
        changed_decision[0].evidence.row_ids[0] = "transfer".into();
        mutations.push((source.clone(), changed_decision, certificate.clone()));
        let mut rejected_change = decisions.clone();
        rejected_change.push(decision(
            "rejected",
            DecisionDisposition::Reject,
            &["fee"],
            &[1],
        ));
        mutations.push((source.clone(), rejected_change, certificate.clone()));
        let mut descriptive_change = source.clone();
        descriptive_change.statement.rows[0].description = Some("changed evidence".into());
        mutations.push((descriptive_change, decisions.clone(), certificate.clone()));
        let mut fabricated = certificate.clone();
        fabricated.residual = Amount(0);
        fabricated.ledger_total = Amount(-11);
        mutations.push((source.clone(), decisions.clone(), fabricated));
        for (input, decision, cert) in mutations {
            assert!(verify_certificate(&input, &decision, &cert).is_err());
        }
    }

    #[test]
    fn duplicate_unresolved_overflow_and_wrong_currency_fail_closed() {
        let mut duplicate = decisions();
        duplicate.push(decision(
            "duplicate",
            DecisionDisposition::Accept,
            &["fee"],
            &[1],
        ));
        assert_eq!(
            build_certificate(&inputs(), &duplicate, false),
            Err(ReconciliationError::DuplicateCoverage)
        );
        let partial = build_certificate(&inputs(), &decisions()[..1], false).unwrap();
        assert!(!partial.closed);
        assert!(!partial.uncovered_rows.is_empty());
        assert_eq!(
            build_certificate(&inputs(), &decisions()[..1], true),
            Err(ReconciliationError::IncompleteClose)
        );
        let mut overflow = inputs();
        overflow.statement.rows = vec![
            row("a", "2026-01-01", i64::MAX, "a"),
            row("b", "2026-01-01", 1, "b"),
        ];
        overflow.statement.total = Amount(0);
        assert_eq!(
            build_certificate(&overflow, &[], false),
            Err(ReconciliationError::ArithmeticOverflow)
        );
        let mut currency = inputs();
        currency.movements[0].currency = "EUR".into();
        assert_eq!(
            generate_candidates(
                &currency,
                CandidateBounds {
                    max_work: 200,
                    max_results: 20,
                    max_cardinality: 2,
                    max_date_distance_days: 2,
                },
            ),
            Err(ReconciliationError::InvalidInput("mixed currency"))
        );
    }

    #[test]
    fn rejection_and_deferral_cover_nothing_and_correction_needs_mia_acceptance() {
        let rejected = decision("no", DecisionDisposition::Reject, &["fee"], &[1]);
        let deferred = decision("later", DecisionDisposition::Defer, &["transfer"], &[2, 3]);
        let certificate =
            build_certificate(&inputs(), &[rejected.clone(), deferred], false).unwrap();
        assert_eq!(certificate.uncovered_rows.len(), 4);
        let postings = vec![
            Posting {
                id: 0,
                source_id: None,
                voucher_id: 0,
                account: 1930,
                amount: Amount(10),
                text: Some("fee reversal".into()),
            },
            Posting {
                id: 0,
                source_id: None,
                voucher_id: 0,
                account: 6570,
                amount: Amount(-10),
                text: Some("fee reversal".into()),
            },
        ];
        let certificate = build_certificate(&inputs(), &decisions(), true).unwrap();
        let draft = prepare_correction_draft(&certificate.accepted[0], postings).unwrap();
        assert!(is_balanced(&draft.postings));
        assert_eq!(
            prepare_correction_draft(&certificate.accepted[0], vec![draft.postings[0].clone()]),
            Err(ReconciliationError::UnbalancedCorrection)
        );
    }
}
