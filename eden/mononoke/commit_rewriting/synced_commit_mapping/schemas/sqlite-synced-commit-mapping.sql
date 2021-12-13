/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- This table has entries for all commits that were actually
-- rewritten from one repo to another. So if a commit was not rewritten
-- (e.g. a large repo commit rewrites into nothingness in a given small repo)
-- then this table will NOT have an entry.
-- This table is not heavily used. The most important use case at the time
-- of writing is figuring out from which repo a rewrite was done (e.g. from
-- small or from large repo).
CREATE TABLE IF NOT EXISTS `synced_commit_mapping` (
  `mapping_id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `small_repo_id` int(11) NOT NULL,
  `small_bcs_id` binary(32) NOT NULL,
  `large_repo_id` int(11) NOT NULL,
  `large_bcs_id` binary(32) NOT NULL,
  `sync_map_version_name` varchar(255),
  -- There is no enum type in SQLite
  `source_repo` varchar(255), -- enum('small','large') DEFAULT NULL,
  UNIQUE (`small_repo_id`,`large_repo_id`,`large_bcs_id`)
);

-- This a mapping between large repo commits and small repo commits.
-- It basically can answer questions like "for this commit from large repo what's the equivalent
-- commit in a given small repo", and the reverse - "for this commit in a given small repo, what's the
-- equivalent large repo commit".
--
-- This is a crucially important table, and it's used for remapping commits between repositories.
-- For example, if we want to sync a commit A from a large repo to a small repo,
-- we can first find a parent P of large repo commit, use synced_working_copy_equivalence
-- to find an equivalent commit in the small repo (P') and rewrite this large repo commit A on top
-- of P'.
--
-- ```
--  A
--  |  \
--  P   \
--  |  \  A'
--  |   \ |
--  B --  P'
--  |     small
--  ..
-- large
--- ```
--
-- Note that both B and P map to the same small repo commit P' - mapping from large commits to small commits is
-- many-to-one. Many large repo commits can rewrite to a single small repo commit,
-- and there are some large repo commits do not have an equivalent in a given small repos.
--
-- A naive implementation (but this is NOT how it's actually implemented!) of this mapping can have `number of large repo commits`
-- times `number of small repos` i.e. for every large repo commit have an equivalent small commit for each small repo (or NULL if
-- there's no equivalent small repo commit). However this is NOT how it's implemented in this table!
-- Some of the NULL entries (i.e. entries that say this large repo commit doesn't have an equivalent small repo commit)
-- are not written, and instead we use mapping from `version_for_large_repo_commit` to work out that this large repo commit shouldn't
-- be rewritten to a given small repo.
-- Another thing to note about `version_for_large_repo_commit` - it stores canonical version of the mapping, so
-- `sync_map_version_name` field in `synced_working_copy_equivalence` table is redundant and could be deleted.
CREATE TABLE IF NOT EXISTS `synced_working_copy_equivalence` (
  `mapping_id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `small_repo_id` int(11) NOT NULL,
  `small_bcs_id` binary(32),
  `large_repo_id` int(11) NOT NULL,
  `large_bcs_id` binary(32) NOT NULL,
  `sync_map_version_name` varchar(255),
   UNIQUE (`large_repo_id`,`small_repo_id`,`large_bcs_id`)
);

 -- Small bcs id can map to multiple large bcs ids
 CREATE INDEX IF NOT EXISTS small_bcs_key ON synced_working_copy_equivalence
  (`large_repo_id`,`small_repo_id`,`small_bcs_id`);

-- This table defines what version should be used to rewrite a given large commit.
-- It's also a crucially important table.
-- As noted in docs for synced_working_copy_equivalence it also defines whether a given
-- large repo commit remaps to a given small repo (i.e. if a given small repo doesn't exists in a given
-- mapping then this large commit shouldn't be rewritten to a given small repo).
CREATE TABLE IF NOT EXISTS `version_for_large_repo_commit` (
  `mapping_id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL ,
  `large_repo_id` int(11) NOT NULL,
  `large_bcs_id` binary(32) NOT NULL,
  `sync_map_version_name` varchar(255) NOT NULL,
  UNIQUE (`large_repo_id`,`large_bcs_id`)
);
