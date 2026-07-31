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
-- The MySQL schema is hand-synced in configerator; `test/main.rs` pins the key shape.
-- `reverse_idx` stays narrow: InnoDB appends the missing PK columns, so the fan-out read covers.
-- Do NOT index `(manifest_repo_id, manifest_branch)` separately -- it is the PK's leftmost prefix.
CREATE TABLE IF NOT EXISTS `repo_manifest_mapping` (
  `manifest_repo_id` INTEGER NOT NULL,
  `manifest_branch` VARBINARY(255) NOT NULL,
  `repo_name` VARBINARY(255) NOT NULL,
  `repo_branch` VARBINARY(255) NOT NULL,
  PRIMARY KEY (`manifest_repo_id`, `manifest_branch`, `repo_name`, `repo_branch`)
);
CREATE INDEX IF NOT EXISTS `reverse_idx` ON `repo_manifest_mapping` (`repo_name`, `repo_branch`);
