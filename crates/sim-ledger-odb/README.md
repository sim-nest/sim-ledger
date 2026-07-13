# sim-ledger-odb

Import helpers for LibreOffice Base ledger exports.

The crate reads the plain HSQLDB `database/script` metadata from an `.odb`,
extracts table layouts and id high-water marks, and maps CSV exports of the
ledger tables into `sim-ledger` source years. It keeps the binary row reader
separate from the import core, so CSV exports and direct `.odb` reads can share
the same accounting checks.
