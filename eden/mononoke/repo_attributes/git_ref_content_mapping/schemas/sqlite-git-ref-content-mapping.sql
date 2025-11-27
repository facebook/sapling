/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- Table for maintaining mapping between git refs and the git trees and blobs that they point to.
-- Note that every other type of ref (i.e. pointing to a commit or tag) is handled through regular
-- Mononoke bookmarks
CREATE TABLE IF NOT EXISTS `git_ref_content_mapping` (
  `repo_id` INT UNSIGNED NOT NULL, -- The ID of the repo for which this ref exists
  `ref_name` VARCHAR(512) NOT NULL, -- The name of the ref
  `git_hash` VARBINARY(20) NOT NULL, -- The Git hash of the object that the ref points to
  `is_tree` BOOLEAN NOT NULL, -- Flag indicating if the object pointed to by the ref is a tree. For refs pointing to blobs, this is set as false 
  PRIMARY KEY (`repo_id`, `ref_name`)
);
