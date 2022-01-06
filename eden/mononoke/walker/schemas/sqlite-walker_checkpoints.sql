/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS walker_checkpoints (
  repo_id INTEGER NOT NULL,
  checkpoint_name VARCHAR(255) NOT NULL,
  lower_bound BIGINT NOT NULL,
  upper_bound BIGINT NOT NULL,
  create_timestamp BIGINT NOT NULL,
  update_timestamp BIGINT NOT NULL,
  update_run_number BIGINT NOT NULL,
  update_chunk_number BIGINT NOT NULL,
  last_finish_timestamp BIGINT NULL,
  last_finish_run_number BIGINT NULL,
  last_finish_chunk_number BIGINT NULL,
  UNIQUE (repo_id, checkpoint_name)
);
