/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS bonsai_hg_mapping (
  repo_id INTEGER NOT NULL,
  hg_cs_id BINARY(20) NOT NULL,
  bcs_id BINARY(32) NOT NULL,
  UNIQUE (repo_id, hg_cs_id),
  PRIMARY KEY (repo_id, bcs_id)
);
