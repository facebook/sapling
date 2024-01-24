/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- Table for maintaining mapping between git annotated tags
-- and the bonsai changeset stored in Mononoke representing the metadata
-- associated with the tag 
CREATE TABLE IF NOT EXISTS bonsai_tag_mapping (
  repo_id INT UNSIGNED NOT NULL, -- The ID of the repo for which this tag exists
  tag_name VARCHAR(512) NOT NULL, -- The name of the tag
  changeset_id VARBINARY(32) NOT NULL, -- The Id of the mapped changeset
  tag_hash VARBINARY(20) DEFAULT 0x0000000000000000000000000000000000000000, -- The Git hash of the tag object
  PRIMARY KEY (repo_id, tag_name)
)
