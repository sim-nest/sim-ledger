# sim-lib-ledger-books

In one line: a review desk for bookkeeping drafts before they enter the books.

## What it gives you

This crate gives ledger workflows a place to check a proposed entry before it is
written into a year. It keeps the money exact, makes the supporting references
visible, and keeps local tax choices in data so tests and hosts can swap the
profile without changing the ledger model.

## Why you will be glad

- Drafts fail early when they do not balance.
- Supporting mail, files, tasks, or vouchers stay as references instead of copied payloads.
- Profile data can be inspected and replaced for local tests.

## Where it fits

This crate sits above the core ledger records. Importers and office bridges can
ask it to check draft entries while the storage crate remains focused on years,
vouchers, postings, and reports.
