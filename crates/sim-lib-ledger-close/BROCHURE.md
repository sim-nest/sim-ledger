# sim-lib-ledger-close

In one line: exact year-end statements from local ledger files.

## What it gives you

This crate closes a fiscal year without changing the accounting facts. It reads
the ledger year, checks that the trial balance nets to zero, groups accounts by
their reporting codes, and returns tables that an office workflow can review or
export.

## Why you will be glad

- Closed years stop accepting ordinary edits.
- Reopen decisions leave a reason in the ledger file.
- Statement totals stay exact because all amounts remain integer minor units.

## Where it fits

This crate sits above the core ledger store. It prepares the accounting view
that office bridges can project into spreadsheets and decks while keeping the
ledger model itself focused on records and storage.
