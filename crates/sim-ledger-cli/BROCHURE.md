# sim-ledger-cli

In one line: a direct terminal path from exported books to checked yearly reports.

## What it gives you

`sim-ledger-cli` lets a person create a ledger set, bring in exported books, see
which years are present, and print balances without writing a custom importer.
It keeps the workflow close to the files on disk, so every step is easy to run
again and easy to inspect.

## Why you will be glad

- Imports and reports use the same checks as the library path.
- Carried numbering is visible after each import.
- A small command surface is enough for repeatable local bookkeeping checks.

## Where it fits

This crate is the terminal face of the ledger tools. It is useful for local
imports, smoke tests, and simple reports while the core crates keep the data
model, database files, and source readers reusable by other SIM surfaces.
