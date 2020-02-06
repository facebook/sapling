/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

CREATE TABLE bonsai_hg_mapping (
  repo_id INTEGER NOT NULL,
  hg_cs_id BINARY(20) NOT NULL,
  bcs_id BINARY(32) NOT NULL,
  UNIQUE (repo_id, hg_cs_id),
  PRIMARY KEY (repo_id, bcs_id)
);
