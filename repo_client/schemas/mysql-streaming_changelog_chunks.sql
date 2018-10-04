CREATE TABLE streaming_changelog_chunks (
  repo_id INT UNSIGNED NOT NULL,
  chunk_num INT UNSIGNED NOT NULL,
  idx_blob_name VARBINARY(4096) NOT NULL,
  idx_size INT UNSIGNED) NOT NULL,
  data_blob_name VARBINARY(4096) NOT NULL,
  data_size INT UNSIGNED) NOT NULL,
  PRIMARY KEY (repo_id,chunk_num)
);