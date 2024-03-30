/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `sparse_profiles_sizes` (
  `cs_id` VARBINARY(32) NOT NULL,
  `profile_name` VARCHAR(512) NOT NULL,
  `size` BIGINT unsigned NOT NULL,
  PRIMARY KEY (`cs_id`, `profile_name`)
)
