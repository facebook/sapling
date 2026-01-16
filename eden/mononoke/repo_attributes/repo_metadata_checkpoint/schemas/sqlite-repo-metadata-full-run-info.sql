/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- Table for tracking when the last full mode run completed for each repo.
-- This is separate from the per-bookmark checkpoint table because full mode
-- runs are a repo-level operation, not a per-bookmark operation.
CREATE TABLE IF NOT EXISTS `repo_metadata_full_run_info` (
  `repo_id` INT UNSIGNED NOT NULL, -- The ID of the repo
  `last_full_run_timestamp` BIGINT NOT NULL, -- Unix timestamp of last successful full run
  PRIMARY KEY (`repo_id`)
);
