/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `megarepo_sync_config` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `bookmark` VARCHAR(512) NOT NULL,
  `version` VARCHAR(512) NOT NULL,
  `serialized_config` TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS `megarepo_sync_config_target_and_version` ON `megarepo_sync_config` (`repo_id`, `bookmark`, `version`);
