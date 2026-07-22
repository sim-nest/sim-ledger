# sim-ledger

Ledger records and exact amount helpers for yearly double-entry books.

The crate defines the shared model: year-local accounts, vouchers, posting
lines, signed amounts stored as hundredths, and a balance check for posting
sets. It has no SIM runtime dependency, so ledger data can be parsed, stored,
reported, or encoded by surrounding crates without pulling in a larger runtime
surface.

## Recipes

The crate ships a generated-doc recipe book at `recipes/book.toml`. The first
recipe, `recipes/01-basics/balanced-year/recipe.toml`, describes the smallest
useful ledger path: two year-local accounts, one voucher, two posting lines, and
an exact zero-sum balance check. It is intentionally synthetic so public docs and
agent cards never need real bookkeeping data.

Use the recipe as the shape to look for when reading the API:

- `Amount` stores signed hundredths, not floating-point values.
- `Voucher` groups the posting lines that must balance together.
- `Account` numbers are year-local, so cross-year reports use reporting codes or
  an explicit mapping instead of assuming a number is global.
- `balances` summarizes postings by account or reporting code without changing
  the source records.
