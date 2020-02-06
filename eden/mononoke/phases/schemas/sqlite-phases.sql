/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

CREATE TABLE phases (
  repo_id INTEGER(11) NOT NULL,
  cs_id VARBINARY(32) NOT NULL,
  --There is no enum type in SQLite
  phase TEXT NOT NULL,
  PRIMARY KEY (repo_id, cs_id)
);
