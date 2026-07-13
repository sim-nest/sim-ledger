PRAGMA foreign_keys = ON;

CREATE TABLE account(
  number    INTEGER PRIMARY KEY,
  name      TEXT NOT NULL,
  note      TEXT,
  sru_plus  INTEGER,
  sru_minus INTEGER
);

CREATE TABLE voucher(
  id        INTEGER PRIMARY KEY,
  source_id INTEGER,
  date      TEXT NOT NULL,
  text      TEXT
);

CREATE TABLE posting(
  id         INTEGER PRIMARY KEY,
  source_id  INTEGER,
  voucher_id INTEGER NOT NULL REFERENCES voucher(id),
  account    INTEGER NOT NULL REFERENCES account(number),
  minor      INTEGER NOT NULL,
  text       TEXT
);

CREATE TABLE id_state(
  kind TEXT PRIMARY KEY,
  next INTEGER NOT NULL
);

CREATE TABLE meta(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
