/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE segmented_changelog_idmap (
  repo_id INTEGER NOT NULL,
  version INTEGER NOT NULL,
  vertex BIGINT NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  PRIMARY KEY (repo_id, version, vertex),
  UNIQUE (repo_id, version, cs_id)
);

CREATE TABLE segmented_changelog_idmap_version (
  repo_id INTEGER PRIMARY KEY,
  version INTEGER NOT NULL
);

CREATE TABLE segmented_changelog_bundle (
  repo_id INTEGER PRIMARY KEY,
  iddag_version VARBINARY(32) NOT NULL,
  idmap_version INTEGER NOT NULL
);

