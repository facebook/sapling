CREATE TABLE phases (
  repo_id INTEGER(11) NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  --There is no enum type in SQLite
  phase TEXT NOT NULL,
  PRIMARY KEY (repo_id, cs_id)
);
