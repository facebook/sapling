/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `bonsai_blob_mapping` (
    `repo_id` INT(11) NOT NULL,
    `cs_id` VARBINARY(32) NOT NULL,
    `blob_key` VARCHAR(255) NOT NULL,
    PRIMARY KEY (`repo_id`, `cs_id`, `blob_key`)
);

CREATE INDEX IF NOT EXISTS `blob_key` ON `bonsai_blob_mapping` (`repo_id`, `blob_key`);
