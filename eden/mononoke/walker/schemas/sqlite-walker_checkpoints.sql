/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE walker_checkpoints (
  repo_id INTEGER NOT NULL,
  checkpoint_name VARCHAR(255) NOT NULL,
  lower_bound BIGINT NOT NULL,
  upper_bound BIGINT NOT NULL,
  create_timestamp BIGINT NOT NULL,
  update_timestamp BIGINT NOT NULL,
  UNIQUE (repo_id, checkpoint_name)
);
