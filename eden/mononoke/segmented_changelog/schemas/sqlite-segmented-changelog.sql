/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * TODO(sfilip): add repo_id and idmap_version
 */
CREATE TABLE segmented_changelog_idmap (
  repo_id INTEGER NOT NULL,
  vertex BIGINT NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  PRIMARY KEY (repo_id, vertex),
  UNIQUE (repo_id, cs_id)
);
