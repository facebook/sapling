/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/* vertex is an older name for dag_id in Mononoke */
CREATE TABLE IF NOT EXISTS segmented_changelog_idmap (
  repo_id INTEGER NOT NULL,
  version INTEGER NOT NULL,
  vertex BIGINT NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  PRIMARY KEY (repo_id, version, vertex),
  UNIQUE (repo_id, version, cs_id)
);

CREATE TABLE IF NOT EXISTS segmented_changelog_version (
  repo_id INTEGER PRIMARY KEY,
  iddag_version VARBINARY(32) NOT NULL,
  idmap_version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS segmented_changelog_idmap_copy_mappings (
  repo_id INTEGER NOT NULL,
  idmap_version INTEGER NOT NULL,
  copied_version INTEGER NOT NULL,
  copy_limit BIGINT NOT NULL,
  PRIMARY KEY (repo_id, copied_version, idmap_version)
);

CREATE TABLE IF NOT EXISTS segmented_changelog_clone_hints (
  repo_id INTEGER NOT NULL,
  idmap_version INTEGER NOT NULL,
  blob_name STRING NOT NULL,
  PRIMARY KEY (repo_id, idmap_version, blob_name)
);
