# sim-lib-ledger-close

Fiscal-year close helpers for `sim-ledger`.

The crate turns year-local ledger postings into trial balances, SRU comparisons,
and exact signed financial statement tables. It also records close/reopen state
in the year file metadata so ordinary ledger insertion APIs refuse closed years.
