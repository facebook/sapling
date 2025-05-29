/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `git_bundles` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `bundle_handle` VARCHAR(255) NOT NULL,
  `bundle_list` INTEGER NOT NULL,
  `in_bundle_list_order` INTEGER NOT NULL,
  `bundle_fingerprint` VARCHAT(255) NOT NULL,
  `generation_start_timestamp` bigint(20) NOT NULL DEFAULT 0,
  UNIQUE (`repo_id`, `bundle_list`, `in_bundle_list_order`)
);
