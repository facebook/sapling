CREATE TABLE phases (
  repo_id INTEGER(11) NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  phase ENUM('Draft','Public') NOT NULL,
  PRIMARY KEY (repo_id, cs_id)
);
