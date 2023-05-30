/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `deletion_log` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` int NOT NULL,
  `cs_id` varbinary(32) NOT NULL,
  `blob_key` varchar(255) NOT NULL,
  `reason` varchar(64) NOT NULL,
  `stage` varchar(10) NOT NULL, -- mysql table has enum type here
  `timestamp` bigint NOT NULL
);

CREATE INDEX IF NOT EXISTS `repo_id_reason` ON deletion_log (`repo_id`, `reason`);
CREATE INDEX IF NOT EXISTS `reason` ON deletion_log (`reason`);
