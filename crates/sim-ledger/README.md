# sim-ledger

Ledger records and exact amount helpers for yearly double-entry books.

The crate defines the shared model: year-local accounts, vouchers, posting
lines, signed amounts stored as hundredths, and a balance check for posting
sets. It has no SIM runtime dependency, so ledger data can be parsed, stored,
reported, or encoded by surrounding crates without pulling in a larger runtime
surface.
