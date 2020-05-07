/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE `reversefillerqueue` (
  `id` INTEGER PRIMARY KEY AUTOINCREMENT,
  `bundle` varchar(255) NOT NULL,
  `reponame` varbinary(255) NOT NULL,
  `slice` UNSIGNED INTEGER DEFAULT '0',
  `created_at` datetime DEFAULT CURRENT_TIMESTAMP,
  `claimed_by` varchar(255) DEFAULT NULL,
  `claimed_at` datetime DEFAULT NULL
);
