/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS commit_graph_backfiller_checkpoints (
  repo_id INTEGER PRIMARY KEY NOT NULL,
  last_finished_id BIGINT NULL
);
