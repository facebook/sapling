/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

CREATE TABLE changesets (
  -- Sqlite doesn't support autoincrement UNSIGNED BIGINT
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INTEGER NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  gen BIGINT NOT NULL,
  UNIQUE (repo_id, cs_id)
);

CREATE TABLE csparents (
  cs_id BIGINT NOT NULL,
  parent_id BIGINT NOT NULL,
  seq INTEGER NOT NULL,
  PRIMARY KEY (cs_id, seq)
);
