/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS changesets (
  -- Sqlite doesn't support autoincrement UNSIGNED BIGINT
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INTEGER NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  gen BIGINT NOT NULL,
  UNIQUE (repo_id, cs_id)
);

CREATE TABLE IF NOT EXISTS csparents (
  cs_id BIGINT NOT NULL,
  parent_id BIGINT NOT NULL,
  seq INTEGER NOT NULL,
  PRIMARY KEY (cs_id, seq)
);
