/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- Table for maintaining information about the metadata logged per repo per bookmark.
-- This will reflect the last successful bonsai changeset that was logged for a given
-- repo and bookmark
CREATE TABLE IF NOT EXISTS `repo_metadata_info` (
  `repo_id` INT UNSIGNED NOT NULL, -- The ID of the repo for which this entry exists
  `bookmark_name` VARCHAR(512) NOT NULL, -- The name of the bookmark
  `changeset_id` VARBINARY(32) NOT NULL, -- The changeset ID that was processed successfully for the given bookmark and repo
  `last_updated_timestamp` BIGINT NOT NULL, -- The unix timestamp of the last successful update
  PRIMARY KEY (`repo_id`, `bookmark_name`)
);
