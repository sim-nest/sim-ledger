# sim-ledger-odb

In one line: bring LibreOffice Base bookkeeping exports into the ledger model without changing the books by hand.

## What it gives you

`sim-ledger-odb` reads the table layout and id counters that LibreOffice Base stores beside a personal ledger, then turns the familiar account, voucher, and posting exports into the shared ledger input format. It keeps the source numbering visible and lets the ledger importer enforce the same balance rules as every other path.

## Why you will be glad

- Existing books can move through a plain export instead of a custom spreadsheet rewrite.
- The source id counters come along, so carried numbering stays auditable.
- The same import checks run whether rows arrive from CSV or direct database reading.

## Where it fits

This crate is the bridge from a LibreOffice Base file to `sim-ledger`. It handles file-shape details and leaves storage, reports, and SIM-facing surfaces to the core ledger crate and surrounding integration crates.
