/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS `pushrebaserecording` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  `repo_id` INTEGER NOT NULL,
  `ontorev` VARBINARY(40) NOT NULL,
  `onto` VARBINARY(512) NOT NULL,
  `pushrebase_errmsg` TEXT,
  `conflicts` TEXT,
  `bundlehandle` TEXT,
  `timestamps` TEXT NOT NULL,
  `replacements_revs` TEXT,
  `ordered_added_revs` TEXT,
  `duration_ms` INTEGER
);
