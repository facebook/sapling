/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `pushrebase_mutation_mapping` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `predecessor_bcs_id` BINARY(32) NOT NULL,
  `successor_bcs_id` BINARY(32) NOT NULL
);

CREATE INDEX IF NOT EXISTS `pushrebase_mutation_mapping_repo_successor_key` ON `pushrebase_mutation_mapping` (`repo_id`, `successor_bcs_id`);

-- Production already carries this index as `repo_predecessor_key`; it is
-- declared here so SQLite-backed tests plan the forward lookup the same way.
CREATE INDEX IF NOT EXISTS `pushrebase_mutation_mapping_repo_predecessor_key` ON `pushrebase_mutation_mapping` (`repo_id`, `predecessor_bcs_id`);
