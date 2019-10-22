CREATE TABLE bonsai_globalrev_mapping (
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INTEGER NOT NULL,
  bcs_id BINARY(32) NOT NULL,
  globalrev INTEGER NOT NULL,
  UNIQUE (repo_id, bcs_id),
  UNIQUE (repo_id, globalrev)
);
