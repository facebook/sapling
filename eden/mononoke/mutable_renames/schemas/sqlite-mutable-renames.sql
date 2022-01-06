/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

CREATE TABLE IF NOT EXISTS mutable_renames(
   `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
   `repo_id` INT UNSIGNED NOT NULL,
   `dst_cs_id` VARBINARY(32) NOT NULL,
   `dst_path_hash` VARBINARY(32) NOT NULL,
   `src_cs_id` VARBINARY(32) NOT NULL,
   `src_path_hash` VARBINARY(32) NOT NULL,
   `src_unode_id` VARBINARY(32) NOT NULL,
   `is_tree` BIT NOT NULL,
   UNIQUE (`repo_id`, `dst_path_hash`, `dst_cs_id`)
);

CREATE TABLE IF NOT EXISTS mutable_renames_paths(
   `id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
   `path_hash` VARBINARY(32) NOT NULL,
   `path` VARBINARY(4096) NOT NULL,
   UNIQUE (`path_hash`)
);
