# sim-ledger-cli

Command line imports and reports for `sim-ledger` sets.

The binary creates ledger-set directories, imports source rows from LibreOffice
Base `.odb` files or CSV export directories, lists imported years, and prints
basic account or SRU balance reports. It uses the same import and report paths
as the library crates, so command line work exercises the storage format that
other integrations read. For `.odb` imports, the required `--year` argument
selects the ledger year; the filename does not need to contain the year.
