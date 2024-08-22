/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `source_of_truth_type` (
  `source_of_truth` VARCHAR(20) PRIMARY KEY NOT NULL,
  `sequence` INTEGER NOT NULL
);

INSERT OR REPLACE INTO `source_of_truth_type` (`source_of_truth`, `sequence`) VALUES ('mononoke', 1), ('metagit', 2), ('locked', 3);

CREATE TABLE IF NOT EXISTS `git_repositories_source_of_truth` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `repo_name` VARCHAR(255) NOT NULL,
  `source_of_truth` VARCHAR(20) NOT NULL DEFAULT ('locked') REFERENCES `source_of_truth_type` (`source_of_truth`)
);

CREATE UNIQUE INDEX IF NOT EXISTS `repo_id_idx` ON `git_repositories_source_of_truth` (`repo_id`);
CREATE UNIQUE INDEX IF NOT EXISTS `repo_name_idx` ON `git_repositories_source_of_truth` (`repo_name`);
CREATE INDEX IF NOT EXISTS `source_of_truth_idx` ON `git_repositories_source_of_truth` (`source_of_truth`);
