/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS phases (
  repo_id INTEGER(11) NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  --There is no enum type in SQLite
  phase TEXT NOT NULL,
  PRIMARY KEY (repo_id, cs_id)
);
