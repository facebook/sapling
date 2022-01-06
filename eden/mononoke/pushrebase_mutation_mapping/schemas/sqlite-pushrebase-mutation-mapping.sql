/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS pushrebase_mutation_mapping (
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INTEGER NOT NULL,
  predecessor_bcs_id BINARY(32) NOT NULL,
  successor_bcs_id BINARY(32) NOT NULL
);

CREATE INDEX IF NOT EXISTS repo_successor_key ON pushrebase_mutation_mapping (repo_id, successor_bcs_id);
