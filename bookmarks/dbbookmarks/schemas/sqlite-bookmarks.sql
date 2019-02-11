CREATE TABLE bookmarks (
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(512) NOT NULL,
  changeset_id VARBINARY(32) NOT NULL,
  PRIMARY KEY (repo_id, name)
);

CREATE TABLE bookmarks_update_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(512) NOT NULL,
  from_changeset_id VARBINARY(32),
  to_changeset_id VARBINARY(32),
  reason VARCHAR(32) NOT NULL, -- enum is used in mysql
  timestamp BIGINT NOT NULL
);

CREATE TABLE bundle_replay_data (
  bookmark_update_log_id INTEGER PRIMARY KEY NOT NULL,
  bundle_handle VARCHAR(256) NOT NULL,
  commit_hashes_json MEDIUMTEXT NOT NULL
);
