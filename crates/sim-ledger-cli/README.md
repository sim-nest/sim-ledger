# sim-ledger-cli

Command line imports, reports, close, and draft checks for `sim-ledger` sets.

The binary creates ledger-set directories, imports source rows from LibreOffice
Base `.odb` files or CSV export directories, lists imported years, and prints
basic account or SRU balance reports. It also closes a year, prints trial
balance and statement tables, compares SRU balances across selected years, and
validates typed journal-draft postings before they are committed elsewhere. It
uses the same import, report, close, and draft-check paths as the library crates,
so command line work exercises the storage format that other integrations read.
For `.odb` imports, the required `--year` argument selects the ledger year; the
filename does not need to contain the year.
