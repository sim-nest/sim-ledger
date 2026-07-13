//! Close and reopen state stored in year-file metadata.

use sim_ledger::{LedgerSet, YearStore};

use crate::CloseError;
use crate::period::ClosingState;
use crate::statements::{FinancialStatements, financial_statements};

const META_CLOSING_STATE: &str = "closing_state";
const META_CLOSING_JOURNAL: &str = "closing_journal";

/// One close/reopen journal entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseJournalEntry {
    /// State after the journaled action.
    pub state: ClosingState,
    /// Operator-facing reason.
    pub reason: String,
}

/// Return the close state for one year.
pub fn close_state(set: &LedgerSet, year: i32) -> Result<ClosingState, CloseError> {
    let store = YearStore::open(&set.year_path(year))?;
    match store.meta_value(META_CLOSING_STATE)? {
        Some(value) => ClosingState::parse(&value),
        None => Ok(ClosingState::Open),
    }
}

/// Close one year and return exact statements for review/export.
pub fn close_year(set: &mut LedgerSet, year: i32) -> Result<FinancialStatements, CloseError> {
    let statements = financial_statements(set, year)?;
    let store = YearStore::open(&set.year_path(year))?;
    store.set_meta(META_CLOSING_STATE, ClosingState::Closed.as_str())?;
    append_journal(&store, ClosingState::Closed, "closed by close_year")?;
    Ok(statements)
}

/// Reopen one year with a journaled reason.
pub fn reopen_year(
    set: &LedgerSet,
    year: i32,
    reason: &str,
) -> Result<Vec<CloseJournalEntry>, CloseError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(CloseError::InvalidState(
            "reopen reason must not be empty".to_owned(),
        ));
    }
    let store = YearStore::open(&set.year_path(year))?;
    store.set_meta(META_CLOSING_STATE, ClosingState::Open.as_str())?;
    append_journal(&store, ClosingState::Open, reason)?;
    close_journal(set, year)
}

/// Read the close/reopen journal for one year.
pub fn close_journal(set: &LedgerSet, year: i32) -> Result<Vec<CloseJournalEntry>, CloseError> {
    let store = YearStore::open(&set.year_path(year))?;
    match store.meta_value(META_CLOSING_JOURNAL)? {
        Some(value) => parse_journal(&value),
        None => Ok(Vec::new()),
    }
}

fn append_journal(store: &YearStore, state: ClosingState, reason: &str) -> Result<(), CloseError> {
    let mut journal = store.meta_value(META_CLOSING_JOURNAL)?.unwrap_or_default();
    if !journal.is_empty() && !journal.ends_with('\n') {
        journal.push('\n');
    }
    journal.push_str(state.as_str());
    journal.push('\t');
    journal.push_str(&reason.replace(['\n', '\t'], " "));
    store.set_meta(META_CLOSING_JOURNAL, &journal)?;
    Ok(())
}

fn parse_journal(value: &str) -> Result<Vec<CloseJournalEntry>, CloseError> {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (state, reason) = line.split_once('\t').ok_or_else(|| {
                CloseError::InvalidState("close journal entry missing separator".to_owned())
            })?;
            Ok(CloseJournalEntry {
                state: ClosingState::parse(state)?,
                reason: reason.to_owned(),
            })
        })
        .collect()
}
