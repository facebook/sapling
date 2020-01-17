CREATE TABLE changesets (
  -- Sqlite doesn't support autoincrement UNSIGNED BIGINT
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INTEGER NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  gen BIGINT NOT NULL,
  UNIQUE (repo_id, cs_id)
);

CREATE TABLE csparents (
  cs_id BIGINT NOT NULL,
  parent_id BIGINT NOT NULL,
  seq INTEGER NOT NULL,
  PRIMARY KEY (cs_id, seq)
);
