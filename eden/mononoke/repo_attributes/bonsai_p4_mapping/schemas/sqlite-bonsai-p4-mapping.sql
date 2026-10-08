/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `bonsai_p4_mapping` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `bcs_id` BINARY(32) NOT NULL,
  `p4_changelist_id` INTEGER NOT NULL,
  UNIQUE (`repo_id`, `bcs_id`),
  UNIQUE (`repo_id`, `p4_changelist_id`)
);
