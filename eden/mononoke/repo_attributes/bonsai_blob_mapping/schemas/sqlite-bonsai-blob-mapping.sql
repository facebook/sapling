/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `bonsai_blob_mapping` (
    `repo_id` int(11) NOT NULL,
    `cs_id` varbinary(32) NOT NULL,
    `blob_key` varchar(255) NOT NULL,
    PRIMARY KEY (`repo_id`, `cs_id`, `blob_key`)
);

CREATE INDEX IF NOT EXISTS `blob_key` ON bonsai_blob_mapping (`repo_id`, `blob_key`);
