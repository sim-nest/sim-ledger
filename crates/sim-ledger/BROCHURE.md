# sim-ledger

In one line: clear yearly books with exact money values and balance checks at the center.

## What it gives you

`sim-ledger` gives SIM a plain model for personal and small-organization accounts: yearly account lists, vouchers, posting lines, and amounts stored as exact hundredths. The pieces are simple enough to inspect directly and strict enough to catch the accounting mistake that matters first, a voucher whose lines do not add back to zero.

## Why you will be glad

- Money is kept as integers, so sums do not drift.
- Accounts belong to one year, matching how real books change over time.
- The balance check is direct and reusable wherever ledger data enters the system.

## Where it fits

This crate is the shared ledger vocabulary. Storage, importers, reports, office documents, and SIM codec surfaces can all pass the same records around instead of inventing their own account and posting shapes. It keeps the bookkeeping core small while leaving the surrounding workflows free to choose their own files, views, and user interfaces.
