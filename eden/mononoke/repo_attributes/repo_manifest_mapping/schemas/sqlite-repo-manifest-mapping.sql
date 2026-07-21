/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

-- Bidirectional `(repo_name, repo_branch) <-> (manifest_repo_id, manifest_branch)`
-- membership projection. "manifest" here means an AOSP/west-style repo-manifest
-- (a `default.xml` listing member repos and their branches), NOT a Mononoke
-- derived-data manifest.
--
-- Keys are git ref names stored as raw bytes so comparisons are byte-exact
-- (i.e. CASE-SENSITIVE). `manifest_repo_id` scopes rows to the manifest repo
-- that owns the manifest branch, so multiple manifest repos (e.g. AOSP and a
-- west/Zephyr firmware manifest) can coexist in one table.
--
-- The production MySQL schema is authored separately in configerator with a
-- binary/`_bin` collation on the VARBINARY columns. Do NOT add a separate index
-- on `(manifest_repo_id, manifest_branch)`: it is the leftmost prefix of the
-- UNIQUE key below, which already serves forward lookups scoped by
-- `(manifest_repo_id, manifest_branch)`.
CREATE TABLE IF NOT EXISTS `repo_manifest_mapping` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `manifest_repo_id` INTEGER NOT NULL,
  `manifest_branch` VARBINARY(255) NOT NULL,
  `repo_name` VARBINARY(255) NOT NULL,
  `repo_branch` VARBINARY(255) NOT NULL,
  UNIQUE (`manifest_repo_id`, `manifest_branch`, `repo_name`, `repo_branch`)
);
CREATE INDEX IF NOT EXISTS `reverse_idx` ON `repo_manifest_mapping` (`repo_name`, `repo_branch`);
