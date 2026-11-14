-- Remembered episode numbering: sequences the user confirmed, and the
-- release numbers each anime in one covers. Not tied to the anime table, so
-- refreshing or deleting cached anime never removes a confirmed numbering.
CREATE TABLE numbering_sequence (
  id INTEGER PRIMARY KEY,
  root INTEGER NOT NULL,
  saved_at INTEGER NOT NULL
);

CREATE TABLE numbering_entry (
  sequence_id INTEGER NOT NULL REFERENCES numbering_sequence (id) ON DELETE CASCADE,
  -- An anime belongs to one sequence at most.
  anime_id INTEGER NOT NULL UNIQUE,
  first INTEGER NOT NULL,
  last INTEGER NOT NULL
);
