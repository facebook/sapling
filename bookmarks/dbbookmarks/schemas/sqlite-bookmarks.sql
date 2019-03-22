CREATE TABLE bookmarks (
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(512) NOT NULL,
  changeset_id VARBINARY(32) NOT NULL,
  publishing tinyint(1) NOT NULL DEFAULT '1', --bookmark can be public or scratch'
  pull_default tinyint(1) NOT NULL DEFAULT '1', --bookmark can be pulled by default or not'
  PRIMARY KEY (repo_id, name)
);

CREATE INDEX repo_id_publishing ON bookmarks (repo_id, publishing);
CREATE INDEX repo_id_pull_default ON bookmarks (repo_id, pull_default);

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
