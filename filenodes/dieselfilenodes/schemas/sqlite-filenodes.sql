CREATE TABLE filenodes (
  repo_id INT UNSIGNED NOT NULL,
  path_hash VARBINARY(32) NOT NULL,
  is_tree BIT NOT NULL,
  filenode BINARY(20) NOT NULL,
  linknode VARBINARY(32) NOT NULL,
  p1 BINARY(20),
  p2 BINARY(20),
  PRIMARY KEY (repo_id, path_hash, is_tree, filenode)
);

CREATE TABLE paths (
  repo_id INT UNSIGNED NOT NULL,
  path_hash VARBINARY(32) NOT NULL,
  path VARBINARY(4096) NOT NULL,
  PRIMARY KEY (repo_id, path_hash)
);
