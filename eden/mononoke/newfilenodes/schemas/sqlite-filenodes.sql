/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS filenodes (
  repo_id INT UNSIGNED NOT NULL,
  path_hash VARBINARY(32) NOT NULL,
  is_tree BIT NOT NULL,
  filenode BINARY(20) NOT NULL,
  linknode VARBINARY(32) NOT NULL,
  p1 BINARY(20),
  p2 BINARY(20),
  has_copyinfo BIT NOT NULL,
  PRIMARY KEY (repo_id, path_hash, is_tree, filenode)
);

CREATE TABLE IF NOT EXISTS fixedcopyinfo (
  repo_id INT UNSIGNED NOT NULL,
  topath_hash VARBINARY(32) NOT NULL,
  tonode BINARY(20) NOT NULL,
  is_tree BIT NOT NULL,
  frompath_hash VARBINARY(32) NOT NULL,
  fromnode BINARY(20) NOT NULL,
  PRIMARY KEY (repo_id, topath_hash, tonode, is_tree)
);

CREATE TABLE IF NOT EXISTS paths (
  repo_id INT UNSIGNED NOT NULL,
  path_hash VARBINARY(32) NOT NULL,
  path VARBINARY(4096) NOT NULL,
  PRIMARY KEY (repo_id, path_hash)
);
