CREATE TABLE bookmarks (
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(512) NOT NULL,
  changeset_id VARBINARY(32) NOT NULL,
  PRIMARY KEY (repo_id, name)
);
