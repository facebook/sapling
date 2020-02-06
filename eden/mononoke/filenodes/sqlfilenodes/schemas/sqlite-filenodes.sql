/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

CREATE TABLE filenodes (
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

CREATE TABLE fixedcopyinfo (
  repo_id INT UNSIGNED NOT NULL,
  topath_hash VARBINARY(32) NOT NULL,
  tonode BINARY(20) NOT NULL,
  is_tree BIT NOT NULL,
  frompath_hash VARBINARY(32) NOT NULL,
  fromnode BINARY(20) NOT NULL,
  PRIMARY KEY (repo_id, topath_hash, tonode, is_tree)
);

CREATE TABLE paths (
  repo_id INT UNSIGNED NOT NULL,
  path_hash VARBINARY(32) NOT NULL,
  path VARBINARY(4096) NOT NULL,
  PRIMARY KEY (repo_id, path_hash)
);
