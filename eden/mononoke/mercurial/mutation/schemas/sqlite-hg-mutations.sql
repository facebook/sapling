/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `hg_mutation_changesets` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER UNSIGNED NOT NULL,
  `changeset_id` BINARY(20) NOT NULL,
  UNIQUE (`repo_id`, `changeset_id`)
);

CREATE TABLE IF NOT EXISTS `hg_mutation_info` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER UNSIGNED NOT NULL,
  `successor` BINARY(20) NOT NULL,
  `primordial` BINARY(20) NOT NULL,
  `pred_count` INTEGER NOT NULL,
  `split_count` INTEGER NOT NULL,
  `op` VARCHAR(32) NOT NULL,
  `user` VARCHAR(512) NOT NULL,
  `timestamp` BIGINT NOT NULL,
  `tz` INTEGER NOT NULL,
  `extra` TEXT, -- JSON '[["key","value"],...]',
  UNIQUE (`repo_id`, `successor`)
);

CREATE INDEX IF NOT EXISTS `hg_mutation_info_repo_id_primordial`
  ON `hg_mutation_info` (`repo_id`, `primordial`);

CREATE TABLE IF NOT EXISTS `hg_mutation_preds` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER UNSIGNED NOT NULL,
  `successor` BINARY(20) NOT NULL,
  `seq` INTEGER NOT NULL,
  `predecessor` BINARY(20) NOT NULL,
  `primordial` BINARY(20) NOT NULL,
  UNIQUE (`repo_id`, `successor`, `seq`)
);

CREATE INDEX IF NOT EXISTS `hg_mutation_preds_repo_id_predecessor`
  ON `hg_mutation_preds` (`repo_id`, `predecessor`);

CREATE INDEX IF NOT EXISTS `hg_mutation_preds_repo_id_primordial`
  ON `hg_mutation_preds` (`repo_id`, `primordial`);

CREATE TABLE IF NOT EXISTS `hg_mutation_splits` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER UNSIGNED NOT NULL,
  `successor` BINARY(20) NOT NULL,
  `seq` INTEGER NOT NULL,
  `split_successor` BINARY(20) NOT NULL,
  UNIQUE (`repo_id`, `successor`, `seq`)
);

CREATE INDEX IF NOT EXISTS `hg_mutation_splits_repo_id_split_successor`
  ON `hg_mutation_splits` (`repo_id`, `split_successor`);
