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
    let certificate = build_certificate(&inputs(), &[rejected.clone(), deferred], false).unwrap();
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
