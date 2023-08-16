/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- Table for maintaining list of symbolic refs (symref) for a repo.
-- Symbolic refs are different from regular refs (branches and tags) and hence are not
-- stored in the bookmarks table. Instead of pointing to a commit, symbolic refs point
-- to another ref within the repository. HEAD is an example of a symref
CREATE TABLE IF NOT EXISTS git_symbolic_refs (
  repo_id INT UNSIGNED NOT NULL, -- The ID of the repo for which this symref exists
  symref_name VARCHAR(512) NOT NULL, -- The name of the symref
  ref_name VARCHAR(512) NOT NULL, -- The name of the ref that is pointed to by the symref
  ref_type VARCHAR(32) NOT NULL DEFAULT (CAST('branch' AS BLOB)), -- The type of the ref, only acceptable values are 'branch' and 'tag'
  PRIMARY KEY (repo_id, symref_name)
)
