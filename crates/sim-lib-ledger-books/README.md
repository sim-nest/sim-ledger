# sim-lib-ledger-books

Bookkeeping journal drafts and profile data for `sim-ledger`.

The crate checks draft vouchers before they are committed into a ledger year.
Drafts use exact posting amounts from `sim-ledger`, carry reference-only
evidence, and load tax or VAT profile choices from data files instead of code.
