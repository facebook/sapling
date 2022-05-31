/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `repo_lock` (
  `repo_id` INTEGER PRIMARY KEY,
  `state` INTEGER NOT NULL,
  `reason` VARCHAR(255)
);
