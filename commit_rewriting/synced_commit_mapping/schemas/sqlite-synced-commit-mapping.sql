CREATE TABLE synced_commit_mapping (
  small_repo_id INTEGER NOT NULL,
  small_bcs_id BINARY(32) NOT NULL,
  large_repo_id INTEGER NOT NULL,
  large_bcs_id BINARY(32) NOT NULL,
  UNIQUE (small_repo_id, small_bcs_id, large_repo_id),
  UNIQUE (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id),
  PRIMARY KEY (small_repo_id, small_bcs_id)
);
