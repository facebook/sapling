/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `restricted_paths_manifest_ids` (
  `repo_id` INT UNSIGNED NOT NULL,
  `manifest_type` VARCHAR(32) NOT NULL,
  `manifest_id` VARBINARY(32) NOT NULL,
  `path` VARBINARY(4096) NOT NULL,
  UNIQUE (`repo_id`, `manifest_type`, `manifest_id`, `path`)
);
