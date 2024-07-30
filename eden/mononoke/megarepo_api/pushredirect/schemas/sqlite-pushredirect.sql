/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

 CREATE TABLE IF NOT EXISTS `pushredirect` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `draft_push` BOOLEAN NOT NULL DEFAULT FALSE,
  `public_push` BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE UNIQUE INDEX IF NOT EXISTS `pushredirect_repo_id` ON `pushredirect` (`repo_id`);
