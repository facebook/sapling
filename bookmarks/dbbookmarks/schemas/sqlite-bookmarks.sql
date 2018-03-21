CREATE TABLE bookmarks (
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(255) NOT NULL,
  changeset_id VARBINARY(32) NOT NULL,
  UNIQUE (repo_id, name)
);
