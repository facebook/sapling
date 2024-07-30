/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `git_push_redirect` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `mononoke` BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE UNIQUE INDEX IF NOT EXISTS `git_push_redirect_repo_id` ON `git_push_redirect` (`repo_id`);
CREATE INDEX IF NOT EXISTS `git_push_redirect_mononoke` ON `git_push_redirect` (`mononoke`);
