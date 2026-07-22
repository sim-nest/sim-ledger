# sim-ledger

In one line: yearly books, imports, draft checks, and close reports with exact money at the center.

## What it gives you

`sim-ledger` gives SIM a plain bookkeeping lane for personal and small-organization accounts. It keeps yearly account lists, vouchers, posting lines, imports from LibreOffice Base exports, draft-entry checks, and close reports in one exact-money vocabulary. The pieces are simple enough to inspect directly and strict enough to catch the accounting mistake that matters first: a voucher whose lines do not add back to zero.

## Why you will be glad

- Money is kept as integers, so sums do not drift.
- Accounts belong to one year, matching how real books change over time.
- Imports preserve source numbering, so checks remain auditable.
- Draft and year-close helpers let office workflows review books before anything live changes.

## Where it fits

This repository is the ledger family for SIM. Storage, importers, reports, office documents, and SIM codec surfaces can all pass the same records around instead of inventing their own account and posting shapes. It keeps the bookkeeping core small while surrounding workflows choose their own files, views, and user interfaces.
