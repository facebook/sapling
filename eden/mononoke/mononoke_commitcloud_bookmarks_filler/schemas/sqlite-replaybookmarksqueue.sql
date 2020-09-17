/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE `replaybookmarksqueue` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  /* NOTE: this lives in a Mercurial DB, so we have repo names, not IDs there */
  `reponame` VARBINARY NOT NULL,
  `bookmark` VARBINARY NOT NULL,
  `node` VARBINARY NOT NULL,
  `bookmark_hash` VARBINARY NOT NULL,
  `created_at` DATETIME DEFAULT CURRENT_TIMESTAMP,
  `synced` INT NOT NULL DEFAULT 0,
  `backfill` INT NOT NULL DEFAULT 0
);
